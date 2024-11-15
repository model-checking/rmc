// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Module for parsing CBMC's JSON output. In general, this output follows
//! the structure (corresponding to [`ParserItem`] below):
//!
//! ```text
//! [
//!     Program,
//!     Message,
//!     ...,
//!     Message,
//!     Result,
//!     Message,
//!     ProverStatus
//! ]
//! ```
//!
//! The parser included in this file reads from buffered input line by line, and
//! determines if an item can be processed after reading certain lines.
//!
//! The rest of code in this file is related to result postprocessing.

// NOTE: This module should be entirely "about" CBMC, so we should need to import
// anything from other modules of this crate, these should only be std + dependencies.
use anyhow::Result;
use console::style;
use pathdiff::diff_paths;
use rustc_demangle::demangle;
use serde::{Deserialize, Deserializer, Serialize};

use std::env;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdout};

const RESULT_ITEM_PREFIX: &str = "  {\n    \"result\":";

/// A parser item is a top-level unit of output from the CBMC json format.
/// See the parser for more information on how they are processed.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ParserItem {
    Program {
        program: String,
    },
    #[serde(rename_all = "camelCase")]
    Message {
        message_text: String,
        message_type: String,
    },
    Result {
        result: Vec<Property>,
    },
    #[serde(rename_all = "camelCase")]
    ProverStatus {
        _c_prover_status: String,
    },
}

/// Struct that is equivalent to `ParserItem::Result`.
///
/// Note: this struct is only used to provide better error messages when there
/// are issues deserializing a `ParserItem::Result`. See `Parser::parse_item`
/// for more details.
#[allow(unused)]
#[derive(Debug, Deserialize)]
struct ResultStruct {
    result: Vec<Property>,
}

/// Struct that represents a single property in the set of CBMC results.
///
/// Note: `reach` is not part of the parsed data, but it's useful to annotate
/// its reachability status.
#[derive(Clone, Debug, Deserialize)]
pub struct Property {
    pub description: String,
    #[serde(rename = "property")]
    pub property_id: PropertyId,
    #[serde(rename = "sourceLocation")]
    pub source_location: SourceLocation,
    pub status: CheckStatus,
    pub reach: Option<CheckStatus>,
    pub trace: Option<Vec<TraceItem>>,
}

/// CBMC's somewhat-ish consistent format for naming properties.
#[derive(Clone, Debug)]
pub struct PropertyId {
    pub fn_name: Option<String>,
    pub class: String,
    pub id: u32,
}

impl Property {
    const CONTRACT_COVER_PROPERTY_CLASS: &'static str = "contract_cover";
    const COVER_PROPERTY_CLASS: &'static str = "cover";
    const COVERAGE_PROPERTY_CLASS: &'static str = "code_coverage";

    pub fn property_class(&self) -> String {
        self.property_id.class.clone()
    }

    /// Returns true if this is an contract_cover property
    pub fn is_contract_cover_property(&self) -> bool {
        self.property_id.class == Self::CONTRACT_COVER_PROPERTY_CLASS
    }

    // Returns true if this is a code_coverage check
    pub fn is_code_coverage_property(&self) -> bool {
        self.property_id.class == Self::COVERAGE_PROPERTY_CLASS
    }

    /// Returns true if this is a cover property
    pub fn is_cover_property(&self) -> bool {
        self.property_id.class == Self::COVER_PROPERTY_CLASS
    }

    pub fn property_name(&self) -> String {
        let class = &self.property_id.class;
        let id = self.property_id.id;
        match &self.property_id.fn_name {
            Some(fn_name) => format!("{fn_name}.{class}.{id}"),
            None => format!("{class}.{id}"),
        }
    }

    pub fn has_property_class_format(string: &str) -> bool {
        string == "NaN" || string.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c == '-')
    }
}

impl<'de> serde::Deserialize<'de> for PropertyId {
    /// Gets all property attributes from the property ID.
    ///
    /// In general, property IDs have the format `<function>.<class>.<counter>`.
    ///
    /// However, there are cases where we only get two attributes:
    ///  * `<class>.<counter>` (the function is a CBMC builtin)
    ///  * `<function>.<counter>` (missing function definition)
    ///
    /// In these cases, we try to determine if the attribute is a function or not
    /// based on its characters (we assume property classes are a combination
    /// of lowercase letters and the characters `_` and `-`). But this is not completely
    /// reliable. CBMC should be able to provide these attributes as separate fields
    /// in the JSON output: <https://github.com/diffblue/cbmc/issues/7069>
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let id_str = String::deserialize(d)?;

        // Handle a special case that doesn't respect the format, and appears at
        // least in the test `tests/expected/dynamic-error-trait/main.rs` with
        // the description "recursion unwinding assertion".
        //
        // As of CBMC 5.74.0, the property ID is `<function>.recursion`.
        // In earlier versions, it would just be `.recursion`.
        if id_str.ends_with(".recursion") {
            let attributes: Vec<&str> = id_str.splitn(2, '.').collect();
            let fn_name = if attributes[0].is_empty() {
                None
            } else {
                Some(format!("{:#}", demangle(attributes[0])))
            };
            return Ok(PropertyId { fn_name, class: "recursion".to_owned(), id: 1 });
        };

        // Split the property name into three from the end, using `.` as the separator
        let property_attributes: Vec<&str> = id_str.rsplitn(3, '.').collect();
        let attributes_tuple = match property_attributes.len() {
            // The general case, where we get all the attributes
            3 => {
                // Since mangled function names may contain `.`, we check if
                // `property_attributes[1]` has the class format. If it doesn't,
                // it means we've split a function name, so we rebuild it and
                // demangle it.
                if Property::has_property_class_format(property_attributes[1]) {
                    let name = format!("{:#}", demangle(property_attributes[2]));
                    (Some(name), property_attributes[1], property_attributes[0])
                } else {
                    let full_name =
                        format!("{}.{}", property_attributes[2], property_attributes[1]);
                    let name = format!("{:#}", demangle(&full_name));
                    (Some(name), "missing_definition", property_attributes[0])
                }
            }
            2 => {
                // The case where `property_attributes[1]` could be a function
                // or a class. If it has the class format, then it's likely a
                // class (functions are usually mangled names which contain many
                // other symbols).
                if Property::has_property_class_format(property_attributes[1]) {
                    (None, property_attributes[1], property_attributes[0])
                } else {
                    let name = format!("{:#}", demangle(property_attributes[1]));
                    (Some(name), "missing_definition", property_attributes[0])
                }
            }
            // The case we don't expect. It's best to fail with an informative message.
            _ => unreachable!("Found property which doesn't have 2 or 3 attributes"),
        };
        // Do more sanity checks, just in case.
        assert!(
            attributes_tuple.2.chars().all(|c| c.is_ascii_digit()),
            "Found property counter that doesn't match number format"
        );
        // Return tuple after converting counter from string into number.
        // Safe to do because we've checked the format earlier.
        let class = String::from(attributes_tuple.1);
        Ok(PropertyId {
            fn_name: attributes_tuple.0,
            class,
            id: attributes_tuple.2.parse().unwrap(),
        })
    }
}

/// Struct that represents a CBMC source location.
///
/// Source locations may be completely empty, which is why
/// all members are optional.
#[derive(Clone, Debug, Deserialize)]
pub struct SourceLocation {
    pub column: Option<String>,
    pub file: Option<String>,
    pub function: Option<String>,
    pub line: Option<String>,
}

impl SourceLocation {
    /// Determines if fundamental parts of a source location are missing.
    pub fn is_missing(&self) -> bool {
        self.file.is_none() && self.function.is_none()
    }
}

/// `Display` implement for `SourceLocation`.
///
/// This is used to format source locations for individual checks. But source
/// locations may be printed in a different way in other places (e.g., in the
/// "Failed Checks" summary at the end).
///
/// Source locations formatted this way will look like:
/// `<file>:<line>:<column> in function <function>`
/// if all attributes were specified. Otherwise, we:
///  * Omit `in function <function>` if the function isn't specified.
///  * Use `Unknown file` instead of `<file>:<line>:<column>` if the file isn't
///    specified.
///  * Lines and columns are only formatted if they were specified and preceding
///    attribute was formatted.
impl std::fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(file) = self.file.clone() {
            let file_path = filepath(file);
            write!(f, "{file_path}")?;
            if let Some(line) = self.line.clone() {
                write!(f, ":{line}")?;
                if let Some(column) = self.column.clone() {
                    write!(f, ":{column}")?;
                }
            }
        } else {
            write!(f, "Unknown file")?;
        }
        if let Some(function) = self.function.clone() {
            let demangled_function = demangle(&function);
            write!(f, " in function {demangled_function:#}")?;
        }
        Ok(())
    }
}

/// Returns a path relative to the current working directory.
fn filepath(file: String) -> String {
    let file_path = PathBuf::from(file.clone());
    let cur_dir = env::current_dir().unwrap();

    let diff_path_opt = diff_paths(file_path, cur_dir);
    if let Some(diff_path) = diff_path_opt {
        diff_path.into_os_string().into_string().unwrap()
    } else {
        file
    }
}

/// Struct that represents traces.
///
/// In general, traces may include more information than this, but this is not
/// documented anywhere. So we ignore the rest for now.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceItem {
    pub step_type: String,
    pub lhs: Option<String>,
    pub source_location: Option<SourceLocation>,
    pub value: Option<TraceValue>,
}

/// Struct that represents a trace value.
///
/// Note: this struct can have a lot of different fields depending on the value type.
/// The fields included right now are relevant to primitive types.
#[derive(Clone, Debug, Deserialize)]
pub struct TraceValue {
    pub binary: Option<String>,
    pub data: Option<TraceData>,
    pub width: Option<u32>,
}

/// Enum that represents a trace data item.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum TraceData {
    NonBool(String),
    Bool(bool),
}

impl std::fmt::Display for TraceData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonBool(trace_data) => write!(f, "{trace_data}"),
            Self::Bool(trace_data) => write!(f, "{trace_data}"),
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum CheckStatus {
    Failure,
    Covered,   // for `code_coverage` properties only
    Satisfied, // for `cover` properties only
    Success,
    Undetermined,
    Unknown,
    Unreachable,
    Uncovered,     // for `code_coverage` properties only
    Unsatisfiable, // for `cover` properties only
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let check_str = match self {
            CheckStatus::Satisfied => style("SATISFIED").green(),
            CheckStatus::Success => style("SUCCESS").green(),
            CheckStatus::Covered => style("COVERED").green(),
            CheckStatus::Uncovered => style("UNCOVERED").red(),
            CheckStatus::Failure => style("FAILURE").red(),
            CheckStatus::Unreachable => style("UNREACHABLE").yellow(),
            CheckStatus::Undetermined => style("UNDETERMINED").yellow(),
            // CBMC 6+ uses UNKNOWN when another property of undefined behavior failed, making it
            // impossible to definitively conclude whether other properties hold or not.
            CheckStatus::Unknown => style("UNDETERMINED").yellow(),
            CheckStatus::Unsatisfiable => style("UNSATISFIABLE").yellow(),
        };
        write!(f, "{check_str}")
    }
}

#[derive(PartialEq)]
enum Action {
    ClearInput,
    ProcessItem,
}

/// A parser for CBMC output, whose state is determined by
/// the input accumulator, required to process items on the fly.
///
/// CBMC's JSON output is defined as a JSON array which contains:
///  1. One program at the beginning (i.e., a message with CBMC's version).
///  2. Messages, which can appears anywhere, and are either status messages or error messages.
///  3. Verification results, another JSON array with all individual checks.
///  4. Prover status, at the end. Because the verification results depends on
///     our postprocessing, this is not used.
///
/// The parser reads the output line by line. A line may trigger one action, and
/// the action may return a parsed item.
///
/// There is a feature request for serde_json which would obsolete this if
/// it ever lands: <https://github.com/serde-rs/json/issues/404>
/// (Would provide a streaming iterator over a json array.)
struct Parser {
    pub input_so_far: String,
}

impl Parser {
    fn new() -> Self {
        Parser { input_so_far: String::new() }
    }

    /// Triggers an action based on the input:
    ///  * Square brackets ('[' and ']') will trigger the `ClearInput` action
    ///    because we assume parsing is done on a JSON array.
    ///  * Curly closing bracket ('}') preceded by two spaces will trigger the
    ///    `ProcessItem` action. The spaces are important in this case because
    ///    assume we're in a JSON array. Matching on this specific string guarantees
    ///    that we'll always get an item when we attempt to process an item.
    ///
    /// This has be updated if the output format changes at some point.
    fn triggers_action(&self, input: String) -> Option<Action> {
        if input.starts_with('[') || input.starts_with(']') {
            // We don't expect any other characters (except '\n') to appear
            // after '[' or ']'. The assert below ensures we won't ignore them.
            assert!(input.len() == 2);
            return Some(Action::ClearInput);
        }
        if input.starts_with("  }") {
            return Some(Action::ProcessItem);
        }
        None
    }

    /// Clears the input accumulated so far.
    fn clear_input(&mut self) {
        self.input_so_far.clear();
    }

    /// Performs an action. In both cases, the input is cleared.
    fn do_action(&mut self, action: Action) -> Option<ParserItem> {
        match action {
            Action::ClearInput => {
                self.clear_input();
                None
            }
            Action::ProcessItem => {
                let item = self.parse_item();
                self.clear_input();
                Some(item)
            }
        }
    }

    // Adds a string to the input accumulated so far
    fn add_to_input(&mut self, input: String) {
        self.input_so_far.push_str(&input);
    }

    // Returns a `ParserItem` from the input we have accumulated so far. Since
    // all items except the last one are delimited (with a comma), we first try
    // to parse the item without the delimiter (i.e., the last character). If
    // that fails, then we parse the item using the whole input.
    fn parse_item(&self) -> ParserItem {
        let string_without_delimiter = &self.input_so_far[0..self.input_so_far.len() - 2];
        let result_item: Result<ParserItem, _> = serde_json::from_str(string_without_delimiter);
        if let Ok(item) = result_item {
            return item;
        }
        // If we failed to parse a `ParserItem::Result` earlier, we will get
        // this error message when we attempt to parse it using the complete
        // string:
        // ```
        // thread '<unnamed>' panicked at 'called `Result::unwrap()` on an `Err` value:
        // Error("data did not match any variant of untagged enum ParserItem", line: 0, column: 0)'
        // ```
        // This error message doesn't provide information about what went wrong
        // while parsing due to `ParserItem` being an untagged enum. A more
        // informative error message will be produced if we attempt to
        // deserialize it into a struct. The attempt will still fail, but it
        // shouldn't be hard to debug with that information. The same strategy
        // can be used for other `ParserItem` variants, but they're normally
        // easier to debug.
        if string_without_delimiter.starts_with(RESULT_ITEM_PREFIX) {
            let result_item: Result<ResultStruct, _> =
                serde_json::from_str(string_without_delimiter);
            result_item.unwrap();
        }
        let complete_string = &self.input_so_far[0..self.input_so_far.len()];
        let result_item: Result<ParserItem, _> = serde_json::from_str(complete_string);
        result_item.unwrap()
    }

    /// Processes a line to determine if an action must be triggered.
    /// The action may result in a `ParserItem`, which is then returned.
    fn process_line(&mut self, input: String) -> Option<ParserItem> {
        self.add_to_input(input.clone());
        let action_required = self.triggers_action(input);
        if let Some(action) = action_required {
            let possible_item = self.do_action(action);
            return possible_item;
        }
        None
    }

    /// Read the process output and return when an item is found in the output
    /// or the EOF is reached
    async fn read_output(
        &mut self,
        buffer: &mut BufReader<&mut ChildStdout>,
    ) -> Option<ParserItem> {
        loop {
            let mut input = String::new();
            match buffer.read_line(&mut input).await {
                Ok(len) => {
                    if len == 0 {
                        return None;
                    }
                    let item = self.process_line(input);
                    if item.is_some() {
                        return item;
                    } else {
                        continue;
                    }
                }
                Err(error) => {
                    panic!("Error: Got error {error} while parsing the output.");
                }
            }
        }
    }
}

/// The verification output, as extracted by the CBMC output parser.
pub struct VerificationOutput {
    pub process_status: i32,
    pub processed_items: Vec<ParserItem>,
}

/// The main function to process CBMC's output.
///
/// This streams CBMC's output to be processed item-by-item with `eager_filter`.
///
/// In general, a filter will pre-process an item (this may or may not transform the item),
/// then formatted (according to the output format) and print.
///
/// The cbmc process status is returned, along with the (post-filter) items.
pub async fn process_cbmc_output(
    process: &mut Child,
    mut eager_filter: impl FnMut(ParserItem) -> Option<ParserItem>,
) -> Result<VerificationOutput> {
    let stdout = process.stdout.as_mut().unwrap();
    let mut stdout_reader = BufReader::new(stdout);
    let mut parser = Parser::new();
    // This should run until stdout is closed (which should mean the process
    // exited) or the specified timeout is reached
    let mut processed_items = Vec::new();
    while let Some(item) = parser.read_output(&mut stdout_reader).await {
        if let Some(item) = eager_filter(item) {
            processed_items.push(item);
        }
    }

    // This will get us the process's exit code
    let status = process.wait().await?;

    let process_status = match (status.code(), status.signal()) {
        // normal unix exit codes (cbmc uses currently 0-10)
        // https://github.com/diffblue/cbmc/blob/develop/src/util/exit_codes.h
        (Some(x), _) => x,
        // process exited with signal (e.g. OOM-killed)
        // bash/zsh have a convention for translating signal number to exit code:
        // https://tldp.org/LDP/abs/html/exitcodes.html
        (_, Some(x)) => 128 + x,
        // I think this shouldn't happen? either exit or signal, right?
        (None, None) => unreachable!("Process exited with neither status code nor signal?"),
    };

    Ok(VerificationOutput { process_status, processed_items })
}

/// Takes (by ownership) a vector of messages, and returns that vector with the `Result`
/// (if any) removed from it and returned separately.
pub fn extract_results(mut items: Vec<ParserItem>) -> (Vec<ParserItem>, Option<Vec<Property>>) {
    let result_idx = items.iter().position(|x| matches!(x, ParserItem::Result { .. }));
    if let Some(result_idx) = result_idx {
        let result = items.remove(result_idx);
        if let ParserItem::Result { result } = result {
            (items, Some(result))
        } else {
            unreachable!() // We filtered for this to be true
        }
    } else {
        // No results
        (items, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_property_id_deserialization_general() {
        let prop_id_string = "\"alloc::raw_vec::RawVec::<u8>::allocate_in.sanity_check.1\"";
        let prop_id_result: Result<PropertyId, serde_json::Error> =
            serde_json::from_str(prop_id_string);
        let prop_id = prop_id_result.unwrap();
        assert_eq!(
            prop_id.fn_name,
            Some(String::from("alloc::raw_vec::RawVec::<u8>::allocate_in"))
        );
        assert_eq!(prop_id.class, String::from("sanity_check"));
        assert_eq!(prop_id.id, 1);

        let dummy_prop = Property {
            description: "".to_string(),
            property_id: prop_id,
            source_location: SourceLocation {
                function: None,
                file: None,
                column: None,
                line: None,
            },
            status: CheckStatus::Success,
            reach: None,
            trace: None,
        };
        assert_eq!(dummy_prop.property_name(), prop_id_string[1..prop_id_string.len() - 1]);
    }

    #[test]
    fn check_property_id_deserialization_only_name() {
        let prop_id_string = "\"alloc::raw_vec::RawVec::<u8>::allocate_in.1\"";
        let prop_id_result: Result<PropertyId, serde_json::Error> =
            serde_json::from_str(prop_id_string);
        dbg!(&prop_id_result);
        let prop_id = prop_id_result.unwrap();
        assert_eq!(
            prop_id.fn_name,
            Some(String::from("alloc::raw_vec::RawVec::<u8>::allocate_in"))
        );
        assert_eq!(prop_id.class, "missing_definition");
        assert_eq!(prop_id.id, 1);

        let dummy_prop = Property {
            description: "".to_string(),
            property_id: prop_id,
            source_location: SourceLocation {
                function: None,
                file: None,
                column: None,
                line: None,
            },
            status: CheckStatus::Success,
            reach: None,
            trace: None,
        };
        assert_eq!(
            dummy_prop.property_name(),
            "alloc::raw_vec::RawVec::<u8>::allocate_in.missing_definition.1"
        );
    }

    #[test]
    fn check_property_id_deserialization_only_class() {
        let prop_id_string = "\"assertion.1\"";
        let prop_id_result: Result<PropertyId, serde_json::Error> =
            serde_json::from_str(prop_id_string);
        let prop_id = prop_id_result.unwrap();
        assert_eq!(prop_id.fn_name, None);
        assert_eq!(prop_id.class, String::from("assertion"));
        assert_eq!(prop_id.id, 1);

        let dummy_prop = Property {
            description: "".to_string(),
            property_id: prop_id,
            source_location: SourceLocation {
                function: None,
                file: None,
                column: None,
                line: None,
            },
            status: CheckStatus::Success,
            reach: None,
            trace: None,
        };
        assert_eq!(dummy_prop.property_name(), prop_id_string[1..prop_id_string.len() - 1]);
    }

    #[test]
    fn check_property_id_deserialization_special() {
        let prop_id_string = "\".recursion\"";
        let prop_id_result: Result<PropertyId, serde_json::Error> =
            serde_json::from_str(prop_id_string);
        let prop_id = prop_id_result.unwrap();
        assert_eq!(prop_id.fn_name, None);
        assert_eq!(prop_id.class, String::from("recursion"));
        assert_eq!(prop_id.id, 1);

        let dummy_prop = Property {
            description: "".to_string(),
            property_id: prop_id,
            source_location: SourceLocation {
                function: None,
                file: None,
                column: None,
                line: None,
            },
            status: CheckStatus::Success,
            reach: None,
            trace: None,
        };
        assert_eq!(dummy_prop.property_name(), "recursion.1");
    }

    #[test]
    fn check_property_id_deserialization_special_name() {
        let prop_id_string = "\"alloc::raw_vec::RawVec::<u8>::allocate_in.recursion\"";
        let prop_id_result: Result<PropertyId, serde_json::Error> =
            serde_json::from_str(prop_id_string);
        let prop_id = prop_id_result.unwrap();
        assert_eq!(
            prop_id.fn_name,
            Some(String::from("alloc::raw_vec::RawVec::<u8>::allocate_in"))
        );
        assert_eq!(prop_id.class, String::from("recursion"));
        assert_eq!(prop_id.id, 1);

        let dummy_prop = Property {
            description: "".to_string(),
            property_id: prop_id,
            source_location: SourceLocation {
                function: None,
                file: None,
                column: None,
                line: None,
            },
            status: CheckStatus::Success,
            reach: None,
            trace: None,
        };
        assert_eq!(
            dummy_prop.property_name(),
            "alloc::raw_vec::RawVec::<u8>::allocate_in.recursion.1"
        );
    }

    #[test]
    #[should_panic]
    fn check_property_id_deserialization_panics() {
        let prop_id_string = "\"not_a_property_ID\"";
        let prop_id_result: Result<PropertyId, serde_json::Error> =
            serde_json::from_str(prop_id_string);
        let _prop_id = prop_id_result.unwrap();
    }

    #[test]
    fn check_trace_value_deserialization_works() {
        let data = format!(
            r#"{{
            "binary": "{:0>1000}",
            "data": "0",
            "name": "integer",
            "type": "unsigned __CPROVER_bitvector[960]",
            "width": 960
        }}"#,
            0
        );
        let trace_value: Result<TraceValue, _> = serde_json::from_str(&data);
        assert!(trace_value.is_ok());
    }

    /// Checks that a valid CBMC "result" item can be deserialized into a
    /// `ParserItem` or `ResultStruct`.
    #[test]
    fn check_result_deserialization_works() {
        let data = r#"{
            "result": [
                {
                    "description": "assertion failed: 1 > 2",
                    "property": "long_function_name.assertion.1",
                    "sourceLocation": {
                        "column": "16",
                        "file": "/home/ubuntu/file.rs",
                        "function": "long_function_name",
                        "line": "815"
                    },
                    "status": "SUCCESS"
                }
            ]
        }"#;
        let parser_item: Result<ParserItem, _> = serde_json::from_str(&data);
        let result_struct: Result<ResultStruct, _> = serde_json::from_str(&data);
        assert!(parser_item.is_ok());
        assert!(result_struct.is_ok());
    }
}
