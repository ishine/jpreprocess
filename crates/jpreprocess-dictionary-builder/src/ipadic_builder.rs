use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    str::FromStr,
    u32,
};

use rayon::prelude::*;

use byteorder::{LittleEndian, WriteBytesExt};
use csv::StringRecord;
use glob::glob;
use log::debug;
use yada::{builder::DoubleArrayBuilder, DoubleArray};

use lindera_core::{
    character_definition::{CharacterDefinitions, CharacterDefinitionsBuilder},
    dictionary_builder::DictionaryBuilder,
    error::LinderaErrorKind,
    file_util::read_utf8_file,
    prefix_dict::PrefixDict,
    unknown_dictionary::parse_unk,
    user_dictionary::UserDictionary,
    word_entry::{WordEntry, WordId},
    LinderaResult,
};

const SIMPLE_USERDIC_FIELDS_NUM: usize = 3;
const SIMPLE_WORD_COST: i16 = -10000;
const SIMPLE_CONTEXT_ID: u16 = 0;
const DETAILED_USERDIC_FIELDS_NUM: usize = 13;

pub struct IpadicBuilder {}

impl IpadicBuilder {
    const UNK_FIELDS_NUM: usize = 11;

    pub fn new() -> Self {
        IpadicBuilder {}
    }
}

impl Default for IpadicBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DictionaryBuilder for IpadicBuilder {
    fn build_dictionary(&self, input_dir: &Path, output_dir: &Path) -> LinderaResult<()> {
        fs::create_dir_all(output_dir)
            .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;

        let chardef = self.build_chardef(input_dir, output_dir)?;
        self.build_unk(input_dir, &chardef, output_dir)?;
        self.build_dict(input_dir, output_dir)?;
        self.build_cost_matrix(input_dir, output_dir)?;

        Ok(())
    }

    fn build_user_dictionary(&self, input_file: &Path, output_file: &Path) -> LinderaResult<()> {
        let parent_dir = match output_file.parent() {
            Some(parent_dir) => parent_dir,
            None => {
                return Err(LinderaErrorKind::Io.with_error(anyhow::anyhow!(
                    "failed to get parent directory of output file"
                )))
            }
        };
        fs::create_dir_all(parent_dir)
            .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;

        let user_dict = self.build_user_dict(input_file)?;

        let mut wtr = io::BufWriter::new(
            File::create(output_file)
                .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?,
        );
        bincode::serialize_into(&mut wtr, &user_dict)
            .map_err(|err| LinderaErrorKind::Serialize.with_error(anyhow::anyhow!(err)))?;
        wtr.flush()
            .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;

        Ok(())
    }

    fn build_chardef(
        &self,
        input_dir: &Path,
        output_dir: &Path,
    ) -> LinderaResult<CharacterDefinitions> {
        let char_def_path = input_dir.join("char.def");
        debug!("reading {:?}", char_def_path);

        let char_def = read_utf8_file(&char_def_path)?;
        let mut char_definitions_builder = CharacterDefinitionsBuilder::default();
        char_definitions_builder.parse(&char_def)?;
        let char_definitions = char_definitions_builder.build();

        let mut chardef_buffer = Vec::new();
        bincode::serialize_into(&mut chardef_buffer, &char_definitions)
            .map_err(|err| LinderaErrorKind::Serialize.with_error(anyhow::anyhow!(err)))?;

        let wtr_chardef_path = output_dir.join(Path::new("char_def.bin"));
        let mut wtr_chardef = io::BufWriter::new(
            File::create(wtr_chardef_path)
                .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?,
        );

        write(&chardef_buffer, &mut wtr_chardef)?;

        wtr_chardef
            .flush()
            .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;

        Ok(char_definitions)
    }

    fn build_unk(
        &self,
        input_dir: &Path,
        chardef: &CharacterDefinitions,
        output_dir: &Path,
    ) -> LinderaResult<()> {
        let unk_data_path = input_dir.join("unk.def");
        debug!("reading {:?}", unk_data_path);

        let unk_data = read_utf8_file(&unk_data_path)?;
        let unknown_dictionary = parse_unk(chardef.categories(), &unk_data, Self::UNK_FIELDS_NUM)?;

        let mut unk_buffer = Vec::new();
        bincode::serialize_into(&mut unk_buffer, &unknown_dictionary)
            .map_err(|err| LinderaErrorKind::Serialize.with_error(anyhow::anyhow!(err)))?;

        let wtr_unk_path = output_dir.join(Path::new("unk.bin"));
        let mut wtr_unk = io::BufWriter::new(
            File::create(wtr_unk_path)
                .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?,
        );
        write(&unk_buffer, &mut wtr_unk)?;
        wtr_unk
            .flush()
            .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;

        Ok(())
    }

    fn build_dict(&self, input_dir: &Path, output_dir: &Path) -> LinderaResult<()> {
        let pattern = if let Some(path) = input_dir.to_str() {
            format!("{}/*.csv", path)
        } else {
            return Err(
                LinderaErrorKind::Io.with_error(anyhow::anyhow!("Failed to convert path to &str."))
            );
        };

        let mut filenames: Vec<PathBuf> = Vec::new();
        for entry in
            glob(&pattern).map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?
        {
            match entry {
                Ok(path) => {
                    if let Some(filename) = path.file_name() {
                        filenames.push(Path::new(input_dir).join(filename));
                    } else {
                        return Err(LinderaErrorKind::Io
                            .with_error(anyhow::anyhow!("failed to get filename")));
                    }
                }
                Err(err) => return Err(LinderaErrorKind::Content.with_error(anyhow::anyhow!(err))),
            }
        }

        let mut rows: Vec<StringRecord> = vec![];
        for filename in filenames {
            debug!("reading {:?}", filename);

            let mut rdr = csv::ReaderBuilder::new()
                .has_headers(false)
                .from_path(filename)
                .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;

            for result in rdr.records() {
                let record = result
                    .map_err(|err| LinderaErrorKind::Content.with_error(anyhow::anyhow!(err)))?;
                rows.push(record);
            }
        }

        let mut normalized_rows: Vec<Vec<String>> = rows
            .into_par_iter()
            .map(|row| {
                row.into_iter()
                    // yeah for EUC_JP and ambiguous unicode 8012 vs 8013
                    // same bullshit as above between for 12316 vs 65374
                    .map(|column| column.to_string().replace('―', "—").replace('～', "〜"))
                    .collect()
            })
            .collect();

        normalized_rows.par_sort_by_key(|row| row.get(0).map(|s| s.to_string()));

        let wtr_da_path = output_dir.join(Path::new("dict.da"));
        let mut wtr_da = io::BufWriter::new(
            File::create(wtr_da_path)
                .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?,
        );

        let wtr_vals_path = output_dir.join(Path::new("dict.vals"));
        let mut wtr_vals = io::BufWriter::new(
            File::create(wtr_vals_path)
                .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?,
        );

        let mut word_entry_map: BTreeMap<String, Vec<WordEntry>> = BTreeMap::new();

        for (row_id, row) in normalized_rows.iter().enumerate() {
            word_entry_map
                .entry(row[0].to_string())
                .or_insert_with(Vec::new)
                .push(WordEntry {
                    word_id: WordId(row_id as u32, true),
                    word_cost: i16::from_str(row[3].trim()).map_err(|_err| {
                        LinderaErrorKind::Parse
                            .with_error(anyhow::anyhow!("failed to parse word_cost"))
                    })?,
                    cost_id: u16::from_str(row[1].trim()).map_err(|_err| {
                        LinderaErrorKind::Parse
                            .with_error(anyhow::anyhow!("failed to parse cost_id"))
                    })?,
                });
        }

        write_words(
            output_dir.join(Path::new("dict.words")).as_path(),
            output_dir.join(Path::new("dict.wordsidx")).as_path(),
            &normalized_rows,
            serialize_lindera_word,
        )?;
        write_words(
            output_dir.join(Path::new("jpreprocess.words")).as_path(),
            output_dir.join(Path::new("jpreprocess.wordsidx")).as_path(),
            &normalized_rows,
            serialize_jpreprocess_word,
        )?;

        let mut id = 0u32;

        let mut keyset: Vec<(&[u8], u32)> = vec![];
        for (key, word_entries) in &word_entry_map {
            let len = word_entries.len() as u32;
            let val = (id << 5) | len; // 27bit for word ID, 5bit for different parts of speech on the same surface.
            keyset.push((key.as_bytes(), val));
            id += len;
        }

        let da_bytes = DoubleArrayBuilder::build(&keyset).ok_or_else(|| {
            LinderaErrorKind::Io.with_error(anyhow::anyhow!("DoubleArray build error."))
        })?;

        write(&da_bytes, &mut wtr_da)?;

        let mut vals_buffer = Vec::new();
        for word_entries in word_entry_map.values() {
            for word_entry in word_entries {
                word_entry
                    .serialize(&mut vals_buffer)
                    .map_err(|err| LinderaErrorKind::Serialize.with_error(anyhow::anyhow!(err)))?;
            }
        }

        write(&vals_buffer, &mut wtr_vals)?;

        wtr_vals
            .flush()
            .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;

        Ok(())
    }

    fn build_cost_matrix(&self, input_dir: &Path, output_dir: &Path) -> LinderaResult<()> {
        let matrix_data_path = input_dir.join("matrix.def");
        debug!("reading {:?}", matrix_data_path);

        let matrix_data = read_utf8_file(&matrix_data_path)?;
        let mut lines_it = matrix_data
            .par_lines()
            .map(|line| {
                line.split_whitespace()
                    .map(i32::from_str)
                    .collect::<Result<Vec<i32>, _>>()
                    .map_err(|err| LinderaErrorKind::Parse.with_error(anyhow::anyhow!(err)))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter();
        let header = lines_it.next().ok_or_else(|| {
            LinderaErrorKind::Content.with_error(anyhow::anyhow!("unknown error"))
        })?;
        let forward_size = header[0] as u32;
        let backward_size = header[1] as u32;
        let len = 2 + (forward_size * backward_size) as usize;
        let mut costs = vec![i16::MAX; len];
        costs[0] = forward_size as i16;
        costs[1] = backward_size as i16;
        for fields in lines_it {
            let forward_id = fields[0] as u32;
            let backward_id = fields[1] as u32;
            let cost = fields[2] as u16;
            costs[2 + (backward_id + forward_id * backward_size) as usize] = cost as i16;
        }

        let wtr_matrix_mtx_path = output_dir.join(Path::new("matrix.mtx"));
        let mut wtr_matrix_mtx = io::BufWriter::new(
            File::create(wtr_matrix_mtx_path)
                .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?,
        );
        let mut matrix_mtx_buffer = Vec::new();
        for cost in costs {
            matrix_mtx_buffer
                .write_i16::<LittleEndian>(cost)
                .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;
        }
        write(&matrix_mtx_buffer, &mut wtr_matrix_mtx)?;

        wtr_matrix_mtx
            .flush()
            .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;

        Ok(())
    }

    fn build_user_dict(&self, input_file: &Path) -> LinderaResult<UserDictionary> {
        debug!("reading {:?}", input_file);

        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_path(input_file)
            .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;

        let mut rows: Vec<StringRecord> = vec![];
        for result in rdr.records() {
            let record =
                result.map_err(|err| LinderaErrorKind::Content.with_error(anyhow::anyhow!(err)))?;
            rows.push(record);
        }
        rows.sort_by_key(|row| row[0].to_string());

        let mut word_entry_map: BTreeMap<String, Vec<WordEntry>> = BTreeMap::new();

        for (row_id, row) in rows.iter().enumerate() {
            let surface = row[0].to_string();
            let word_cost = if row.len() == SIMPLE_USERDIC_FIELDS_NUM {
                SIMPLE_WORD_COST
            } else {
                row[3].parse::<i16>().map_err(|_err| {
                    LinderaErrorKind::Parse.with_error(anyhow::anyhow!("failed to parse word cost"))
                })?
            };
            let cost_id = if row.len() == SIMPLE_USERDIC_FIELDS_NUM {
                SIMPLE_CONTEXT_ID
            } else {
                row[1].parse::<u16>().map_err(|_err| {
                    LinderaErrorKind::Parse
                        .with_error(anyhow::anyhow!("failed to parse left context id"))
                })?
            };

            word_entry_map
                .entry(surface)
                .or_insert_with(Vec::new)
                .push(WordEntry {
                    word_id: WordId(row_id as u32, false),
                    word_cost,
                    cost_id,
                });
        }

        let mut words_data = Vec::<u8>::new();
        let mut words_idx_data = Vec::<u8>::new();
        for row in rows.iter() {
            let word_detail = if row.len() == SIMPLE_USERDIC_FIELDS_NUM {
                vec![
                    row[1].to_string(), // POS
                    "*".to_string(),    // POS subcategory 1
                    "*".to_string(),    // POS subcategory 2
                    "*".to_string(),    // POS subcategory 3
                    "*".to_string(),    // Conjugation type
                    "*".to_string(),    // Conjugation form
                    row[0].to_string(), // Base form
                    row[2].to_string(), // Reading
                    "*".to_string(),    // Pronunciation
                ]
            } else if row.len() >= DETAILED_USERDIC_FIELDS_NUM {
                let mut tmp_word_detail = Vec::new();
                for item in row.iter().skip(4) {
                    tmp_word_detail.push(item.to_string());
                }
                tmp_word_detail
            } else {
                return Err(LinderaErrorKind::Content.with_error(anyhow::anyhow!(
                    "user dictionary should be a CSV with {} or {}+ fields",
                    SIMPLE_USERDIC_FIELDS_NUM,
                    DETAILED_USERDIC_FIELDS_NUM
                )));
            };

            let offset = words_data.len();
            words_idx_data
                .write_u32::<LittleEndian>(offset as u32)
                .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;
            bincode::serialize_into(&mut words_data, &word_detail)
                .map_err(|err| LinderaErrorKind::Serialize.with_error(anyhow::anyhow!(err)))?;
        }

        let mut id = 0u32;

        // building da
        let mut keyset: Vec<(&[u8], u32)> = vec![];
        for (key, word_entries) in &word_entry_map {
            let len = word_entries.len() as u32;
            let val = (id << 5) | len;
            keyset.push((key.as_bytes(), val));
            id += len;
        }

        let da_bytes = DoubleArrayBuilder::build(&keyset).ok_or_else(|| {
            LinderaErrorKind::Io
                .with_error(anyhow::anyhow!("DoubleArray build error for user dict."))
        })?;

        // building values
        let mut vals_data = Vec::<u8>::new();
        for word_entries in word_entry_map.values() {
            for word_entry in word_entries {
                word_entry
                    .serialize(&mut vals_data)
                    .map_err(|err| LinderaErrorKind::Serialize.with_error(anyhow::anyhow!(err)))?;
            }
        }

        let dict = PrefixDict {
            da: DoubleArray::new(da_bytes),
            vals_data,
            is_system: false,
        };

        Ok(UserDictionary {
            dict,
            words_idx_data,
            words_data,
        })
    }
}

fn write_words<F>(
    wtr_words_path: &Path,
    wtr_words_idx_path: &Path,
    normalized_rows: &Vec<Vec<String>>,
    f: F,
) -> Result<(), lindera_core::error::LinderaError>
where
    F: Sync + Send + Fn(&Vec<String>) -> Result<Vec<u8>, lindera_core::error::LinderaError>,
{
    let mut wtr_words = io::BufWriter::new(
        File::create(wtr_words_path)
            .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?,
    );
    let mut wtr_words_idx = io::BufWriter::new(
        File::create(wtr_words_idx_path)
            .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?,
    );

    let words = normalized_rows
        .par_iter()
        .map(f)
        .collect::<Result<Vec<Vec<u8>>, _>>()?;
    let words_idx: Vec<usize> = words
        .iter()
        .scan(0, |acc, e| {
            let offset = *acc;
            *acc += e.len();
            Some(offset)
        })
        .collect();
    let mut words_idx_buffer = Vec::with_capacity(words_idx.len() * 4);
    for word_idx in words_idx {
        words_idx_buffer
            .write_u32::<LittleEndian>(word_idx as u32)
            .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;
    }
    let words_buffer = words.concat();

    write(&words_buffer, &mut wtr_words)?;
    write(&words_idx_buffer, &mut wtr_words_idx)?;
    wtr_words
        .flush()
        .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;
    wtr_words_idx
        .flush()
        .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;
    Ok(())
}

fn serialize_lindera_word(row: &Vec<String>) -> Result<Vec<u8>, lindera_core::error::LinderaError> {
    let mut word_detail = Vec::new();
    for item in row.iter().skip(4) {
        word_detail.push(item.to_string());
    }
    bincode::serialize(&word_detail)
        .map_err(|err| LinderaErrorKind::Serialize.with_error(anyhow::anyhow!(err)))
}

fn serialize_jpreprocess_word(
    row: &Vec<String>,
) -> Result<Vec<u8>, lindera_core::error::LinderaError> {
    use jpreprocess_core::word_entry::WordEntry;
    let mut str_details = row.iter().skip(4).map(|d| &d[..]).collect::<Vec<&str>>();
    str_details.resize(13, "");
    let entry = WordEntry::load(&str_details[..])
        .map_err(|err| LinderaErrorKind::Serialize.with_error(anyhow::anyhow!(err)))?;
    bincode::serialize(&entry)
        .map_err(|err| LinderaErrorKind::Serialize.with_error(anyhow::anyhow!(err)))
}

fn write<W: Write>(buffer: &[u8], writer: &mut W) -> LinderaResult<()> {
    writer
        .write_all(buffer)
        .map_err(|err| LinderaErrorKind::Io.with_error(anyhow::anyhow!(err)))?;

    Ok(())
}
