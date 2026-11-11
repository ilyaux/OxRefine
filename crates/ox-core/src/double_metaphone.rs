/*
 * Licensed to the Apache Software Foundation (ASF) under one or more
 * contributor license agreements.  See the NOTICE file distributed with
 * this work for additional information regarding copyright ownership.
 * The ASF licenses this file to You under the Apache License, Version 2.0
 * (the "License"); you may not use this file except in compliance with
 * the License.  You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
// Vendored from rphonetic 4.0.0 (src/double_metaphone.rs, Apache License 2.0), a port of Apache Commons
// Codec's DoubleMetaphone. Changed for OxRefine: rphonetic indexes the uppercased input by bytes, which
// breaks on non-ASCII letters (it panics on "Æsir"). Before encoding, the input is now made one byte per
// character: 'Ç' and 'Ñ' (the only non-ASCII letters Commons Codec encodes) become the markers \u{1}
// and \u{2}, every other non-ASCII character \u{3}, which, like in Commons Codec, encodes nothing.
// The serde derives and the Encoder impl were dropped.
#![allow(dead_code, clippy::all)]

use std::fmt::{Display, Formatter};
use std::iter::Peekable;
use std::str::CharIndices;



fn is_vowel(c: Option<char>, include_y: bool) -> bool {
    match c {
        Some(ch) => matches!(ch, 'a' | 'e' | 'i' | 'o' | 'u') || (include_y && ch == 'y'),
        None => false,
    }
}

const SILENT_START: &[&str; 5] = &["GN", "KN", "PN", "WR", "PS"];
const L_R_N_M_B_H_F_V_W_SPACE: &[&str; 10] = &["L", "R", "N", "M", "B", "H", "F", "V", "W", " "];
const ES_EP_EB_EL_EY_IB_IL_IN_IE_EI_ER: &[&str; 11] = &[
    "ES", "EP", "EB", "EL", "EY", "IB", "IL", "IN", "IE", "EI", "ER",
];
const L_T_K_S_N_M_B_Z: &[&str; 8] = &["L", "T", "K", "S", "N", "M", "B", "Z"];

/// This struct represents a double metaphone result.
/// It contains both `primary` and `alternate` code.
#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct DoubleMetaphoneResult {
    primary: String,
    alternate: String,
    max_length: Option<usize>,
}

impl Display for DoubleMetaphoneResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[primary={}, alternate={}]",
            self.primary, self.alternate
        )
    }
}

/// This is representing a [DoubleMetaphone] result.
///
/// It contains both `primary` and `alternate` codes.
impl DoubleMetaphoneResult {
    fn new(max_length: Option<usize>) -> Self {
        // If no `max_length` is given, allocate resulting string with
        // a capacity of 10. It should be sufficient without realloc.
        Self {
            primary: String::with_capacity(max_length.unwrap_or(10)),
            alternate: String::with_capacity(max_length.unwrap_or(10)),
            max_length,
        }
    }

    /// Return the `primary` code.
    pub fn primary(&self) -> String {
        self.primary.clone()
    }

    /// Return the `alternate` code.
    pub fn alternate(&self) -> String {
        self.alternate.clone()
    }

    fn append_char(&mut self, ch: char, alternate: Option<char>) {
        self.append_char_primary(ch);
        self.append_char_alternate(alternate.unwrap_or(ch));
    }

    fn append_char_primary(&mut self, ch: char) {
        if self
            .max_length
            .map(|v| self.primary.len() < v)
            .unwrap_or(true)
        {
            self.primary.push(ch);
        }
    }

    fn append_char_alternate(&mut self, ch: char) {
        if self
            .max_length
            .map(|v| self.alternate.len() < v)
            .unwrap_or(true)
        {
            self.alternate.push(ch);
        }
    }

    fn append_str(&mut self, value: &str, alternate: Option<&str>) {
        self.append_str_primary(value);
        self.append_str_alternate(alternate.unwrap_or(value));
    }

    fn append_str_primary(&mut self, value: &str) {
        let length_remaining = self.max_length.map(|v| v - self.primary.len());
        if let Some(length_remaining) = length_remaining {
            if value.len() <= length_remaining {
                self.primary.push_str(value);
            } else {
                self.primary.push_str(&value[0..length_remaining]);
            }
        } else {
            self.primary.push_str(value);
        }
    }

    fn append_str_alternate(&mut self, value: &str) {
        let length_remaining = self.max_length.map(|v| v - self.alternate.len());
        if let Some(length_remaining) = length_remaining {
            if value.len() <= length_remaining {
                self.alternate.push_str(value);
            } else {
                self.alternate.push_str(&value[0..length_remaining]);
            }
        } else {
            self.alternate.push_str(value);
        }
    }

    /// It complete when we reach the max size. If no max size is given
    /// then we return `false` so we consume all the string.
    /// https://github.com/Dalvany/rphonetic/issues/49
    fn is_complete(&self) -> bool {
        self.max_length
            .map(|v| self.primary.len() >= v && self.alternate.len() >= v)
            .unwrap_or(false)
    }
}

/// This is the [Double Metaphone](https://en.wikipedia.org/wiki/Metaphone#Double_Metaphone) implementation.
///
/// The [Default] implementation has a maximum code length of 4.
/// Use `new()` contructor to override.
///
/// Double Metaphone can generate two codes: `primary` and `alternate`.
/// [Encoder] implementation returns the primary code while `encode_alternate()` returns `alternate` code.
///
/// # Example
///
/// ```rust
/// use rphonetic::{DoubleMetaphone, Encoder};
///
/// let double_metaphone = DoubleMetaphone::default();
///
/// assert_eq!(double_metaphone.encode("jumped"), "JMPT");
/// assert_eq!(double_metaphone.encode_alternate("jumped"), "AMPT");
/// ```
#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct DoubleMetaphone {
    max_code_length: Option<usize>,
}

impl Default for DoubleMetaphone {
    /// Construct a new [DoubleMetaphone] with a maximum code length of 4.
    fn default() -> Self {
        Self {
            max_code_length: Some(4),
        }
    }
}

impl DoubleMetaphone {
    /// Construct a new [DoubleMetaphone] with the maximum code length provided.
    ///
    /// # Parameter
    ///
    /// * `max_code_length`: the maximum code length. If you provide [Option::None]
    ///   then the resulting code can be of any length.
    pub fn new(max_code_length: Option<usize>) -> Self {
        Self { max_code_length }
    }

    /// This method encode and return the alternate code.
    ///
    /// # Parameter
    ///
    /// * `value` : value to encode.
    ///
    /// # Result
    ///
    /// Alternate value's code.
    pub fn encode_alternate(&self, value: &str) -> String {
        self.double_metaphone(value).alternate
    }

    /// This method check if code generated by `value1` and `value2` are equals.
    ///
    /// # Parameters
    ///
    /// * `value1` and `value2`: values to check.
    /// *  `alternate`: if  ̀false` then `primary` codes are checked, otherwise it is the alternate codes that are compared.
    ///
    /// # Result
    ///
    /// Return `true` if both codes are equals.
    pub fn is_double_metaphone_equal(&self, value1: &str, value2: &str, alternate: bool) -> bool {
        let result1 = self.double_metaphone(value1);
        let result2 = self.double_metaphone(value2);
        if alternate {
            result1.alternate == result2.alternate
        } else {
            result1.primary == result2.primary
        }
    }

    fn is_slavo_germanic(value: &str) -> bool {
        value.chars().any(|c| c == 'W' || c == 'K')
            || value.contains("CZ")
            || value.contains("WITZ")
    }

    fn contains(value: &str, start: isize, length: usize, criteria: Vec<&str>) -> bool {
        let result = false;

        if start < 0 {
            return false;
        }

        let start = start as usize;

        if start + length <= value.len() {
            let target: &str = &value[start..start + length];
            return criteria.contains(&target);
        }

        result
    }

    fn contains_array(value: &str, start: isize, length: usize, criteria: &[&str]) -> bool {
        let result = false;

        if start < 0 {
            return false;
        }
        let start = start as usize;

        if start + length <= value.len() {
            let target: &str = &value[start..start + length];
            return criteria.contains(&target);
        }

        result
    }

    fn char_at(value: &str, index: isize) -> Option<char> {
        if index >= 0 && (index as usize) < value.len() {
            return value[index as usize..].chars().next();
        }

        None
    }

    /// Encode `value` and return the code.
    /// If  ̀alternate` is `false`, then `primary` code
    /// is returned, otherwise it will be the `alternate` code.
    ///
    /// # Parameter
    ///
    /// * `value` : value to encode.
    ///
    /// # Result
    ///
    /// A [DoubleMetaphone] that contains both `primary` and `alternate` code.
    pub fn double_metaphone(&self, value: &str) -> DoubleMetaphoneResult {
        let mut result = DoubleMetaphoneResult::new(self.max_code_length);
        let value = value.trim();
        if value.is_empty() {
            return result;
        }

        let value: &String = &value
            .to_uppercase()
            .chars()
            .map(|c| match c {
                'Ç' => '\u{1}',
                'Ñ' => '\u{2}',
                c if c.is_ascii() => c,
                _ => '\u{3}',
            })
            .collect();

        let slavo_germanic = Self::is_slavo_germanic(value);

        let mut iterator: Peekable<CharIndices<'_>> = value.char_indices().peekable();
        let mut char_index: Option<(usize, char)> = iterator.next();
        if SILENT_START.iter().any(|sl| value.starts_with(sl)) {
            char_index = iterator.next();
        }

        while !result.is_complete() && char_index.is_some() {
            if let Some((index, ch)) = char_index {
                let index = index as isize;

                let skip = match ch {
                    'A' | 'E' | 'I' | 'O' | 'U' | 'Y' => {
                        if index == 0 {
                            result.append_char('A', None);
                        }
                        0
                    }
                    'B' => {
                        result.append_char('P', None);
                        if Self::char_at(value, index + 1) == Some('B') {
                            1
                        } else {
                            0
                        }
                    }
                    '\u{1}' => {
                        result.append_char('S', None);
                        0
                    }
                    'C' => Self::handle_c(value, &mut result, index),
                    'D' => Self::handle_d(value, &mut result, index),
                    'F' => {
                        result.append_char('F', None);
                        if Self::char_at(value, index + 1) == Some('F') {
                            1
                        } else {
                            0
                        }
                    }
                    'G' => Self::handle_g(value, &mut result, index, slavo_germanic),
                    'H' => Self::handle_h(value, &mut result, index),
                    'J' => Self::handle_j(value, &mut result, index, slavo_germanic),
                    'K' => {
                        result.append_char('K', None);
                        if Self::char_at(value, index + 1) == Some('K') {
                            1
                        } else {
                            0
                        }
                    }
                    'L' => Self::handle_l(value, &mut result, index),
                    'M' => {
                        result.append_char('M', None);
                        if Self::condition_m0(value, index) {
                            1
                        } else {
                            0
                        }
                    }
                    'N' => {
                        result.append_char('N', None);
                        if Self::char_at(value, index + 1) == Some('N') {
                            1
                        } else {
                            0
                        }
                    }
                    '\u{2}' => {
                        result.append_char('N', None);
                        0
                    }
                    'P' => Self::handle_p(value, &mut result, index),
                    'Q' => {
                        result.append_char('K', None);
                        if Self::char_at(value, index + 1) == Some('Q') {
                            1
                        } else {
                            0
                        }
                    }
                    'R' => Self::handle_r(value, &mut result, index, slavo_germanic),
                    'S' => Self::handle_s(value, &mut result, index, slavo_germanic),
                    'T' => Self::handle_t(value, &mut result, index),
                    'V' => {
                        result.append_char('F', None);
                        if Self::char_at(value, index + 1) == Some('V') {
                            1
                        } else {
                            0
                        }
                    }
                    'W' => Self::handle_w(value, &mut result, index),
                    'X' => Self::handle_x(value, &mut result, index),
                    'Z' => Self::handle_z(value, &mut result, index, slavo_germanic),
                    _ => 0,
                };

                char_index = iterator.nth(skip);
            } else {
                return result;
            }
        }

        result
    }

    fn handle_c(value: &str, result: &mut DoubleMetaphoneResult, index: isize) -> usize {
        if Self::condition_c0(value, index) {
            result.append_char('K', None);
            1
        } else if index == 0 && Self::contains(value, index, 6, vec!["CAESAR"]) {
            result.append_char('S', None);
            1
        } else if Self::contains(value, index, 2, vec!["CH"]) {
            Self::handle_ch(value, result, index)
        } else if Self::contains(value, index, 2, vec!["CZ"])
            && (index < 2 || !Self::contains(value, index - 2, 4, vec!["WICZ"]))
        {
            //-- "Czerny" --//
            result.append_char('S', Some('X'));
            1
        } else if Self::contains(value, index + 1, 3, vec!["CIA"]) {
            //-- "focaccia" --//
            result.append_char('X', None);
            2
        } else if Self::contains(value, index, 2, vec!["CC"])
            && !(index == 1 && Self::char_at(value, 0) == Some('M'))
        {
            //-- double "cc" but not "McClelland" --//
            Self::handle_cc(value, result, index)
        } else if Self::contains(value, index, 2, vec!["CK", "CG", "CQ"]) {
            result.append_char('K', None);
            1
        } else if Self::contains(value, index, 2, vec!["CI", "CE", "CY"]) {
            //-- Italian vs. English --//
            if Self::contains(value, index, 3, vec!["CIO", "CIE", "CIA"]) {
                result.append_char('S', Some('X'));
            } else {
                result.append_char('S', None);
            }
            1
        } else {
            result.append_char('K', None);
            if Self::contains(value, index + 1, 2, vec![" C", " Q", " G"]) {
                //-- Mac Caffrey, Mac Gregor --//
                2
            } else if Self::contains(value, index + 1, 1, vec!["C", "K", "Q"])
                && !Self::contains(value, index + 1, 2, vec!["CE", "CI"])
            {
                1
            } else {
                0
            }
        }
    }

    fn condition_c0(value: &str, index: isize) -> bool {
        if Self::contains(value, index, 4, vec!["CHIA"]) {
            return true;
        }
        if index < 1 {
            return false;
        }
        if index < 2
            || Self::char_at(value, index - 2).map_or(false, |ch| {
                is_vowel(Some(ch).map(|c| c.to_ascii_lowercase()), true)
            })
        {
            return false;
        }

        if index > 0 && !Self::contains(value, index - 1, 3, vec!["ACH"]) {
            return false;
        }

        let ch = Self::char_at(value, index + 2);
        if index < 2 {
            false
        } else {
            ch.map_or(true, |c| c != 'I' && c != 'E')
                || Self::contains(value, index - 2, 6, vec!["BACHER", "MACHER"])
        }
    }

    fn handle_ch(value: &str, result: &mut DoubleMetaphoneResult, index: isize) -> usize {
        if index > 0 && Self::contains(value, index, 4, vec!["CHAE"]) {
            // Michael
            result.append_char('K', Some('X'));
        } else if Self::condition_ch0(value, index) || Self::condition_ch1(value, index) {
            //-- Greek roots ("chemistry", "chorus", etc.) --//
            //-- Germanic, Greek, or otherwise 'ch' for 'kh' sound --//
            result.append_char('K', None);
        } else if index > 0 {
            if Self::contains(value, 0, 2, vec!["MC"]) {
                result.append_char('K', None);
            } else {
                result.append_char('X', Some('K'));
            }
        } else {
            result.append_char('X', None);
        }

        1
    }

    fn condition_ch0(value: &str, index: isize) -> bool {
        if index != 0 {
            return false;
        }

        if !Self::contains(value, index + 1, 5, vec!["HARAC", "HARIS"])
            && !Self::contains(value, index + 1, 3, vec!["HOR", "HYM", "HIA", "HEM"])
        {
            return false;
        }

        !Self::contains(value, 0, 5, vec!["CHORE"])
    }

    fn condition_ch1(value: &str, index: isize) -> bool {
        (Self::contains(value, 0, 4, vec!["VAN", "VON"])
            || Self::contains(value, 0, 3, vec!["SCH"]))
            || (index > 1
                && Self::contains(value, index - 2, 6, vec!["ORCHES", "ARCHIT", "ORCHID"]))
            || (index > 1 && Self::contains(value, index + 2, 1, vec!["T", "S"]))
            || ((index == 0 || Self::contains(value, index - 1, 1, vec!["A", "O", "U", "E"]))
                && (Self::contains_array(value, index + 2, 1, L_R_N_M_B_H_F_V_W_SPACE)
                    || (index as usize) + 1 == value.len() - 1))
    }

    fn handle_cc(value: &str, result: &mut DoubleMetaphoneResult, index: isize) -> usize {
        if Self::contains(value, index + 2, 1, vec!["I", "E", "H"])
            && !Self::contains(value, index + 2, 2, vec!["HU"])
        {
            //-- "bellocchio" but not "bacchus" --//
            if (index == 1 && Self::char_at(value, index - 1) == Some('A'))
                || Self::contains(value, index - 1, 5, vec!["UCCEE", "UCCES"])
            {
                //-- "accident", "accede", "succeed" --//
                result.append_str("KS", None);
            } else {
                //-- "bacci", "bertucci", other Italian --//
                result.append_char('X', None);
            }
            2
        } else {
            // Pierce's rule
            result.append_char('K', None);
            1
        }
    }

    fn handle_d(value: &str, result: &mut DoubleMetaphoneResult, index: isize) -> usize {
        if Self::contains(value, index, 2, vec!["DG"]) {
            if Self::contains(value, index + 2, 1, vec!["I", "E", "Y"]) {
                result.append_char('J', None);
                2
            } else {
                result.append_str("TK", None);
                1
            }
        } else if Self::contains(value, index, 2, vec!["DT", "DD"]) {
            result.append_char('T', None);
            1
        } else {
            result.append_char('T', None);
            0
        }
    }

    fn handle_g(
        value: &str,
        result: &mut DoubleMetaphoneResult,
        index: isize,
        slavo_germanic: bool,
    ) -> usize {
        if Self::char_at(value, index + 1) == Some('H') {
            Self::handle_gh(value, result, index)
        } else if Self::char_at(value, index + 1) == Some('N') {
            if index == 1
                && is_vowel(
                    Self::char_at(value, 0).map(|c| c.to_ascii_lowercase()),
                    true,
                )
                && !slavo_germanic
            {
                result.append_str("KN", Some("N"));
            } else if !Self::contains(value, index + 2, 2, vec!["EY"])
                && Self::char_at(value, index + 1) != Some('Y')
                && !slavo_germanic
            {
                result.append_str("N", Some("KN"));
            } else {
                result.append_str("KN", None);
            }
            1
        } else if Self::contains(value, index + 1, 2, vec!["LI"]) && !slavo_germanic {
            result.append_str("KL", Some("L"));
            1
        } else if (index == 0
            && (Self::char_at(value, index + 1) == Some('Y')
                || Self::contains_array(value, index + 1, 2, ES_EP_EB_EL_EY_IB_IL_IN_IE_EI_ER)))
            || (Self::contains(value, index + 1, 2, vec!["ER"])
                || Self::char_at(value, index + 1) == Some('Y'))
                && !Self::contains(value, 0, 6, vec!["DANGER", "RANGER", "MANGER"])
                && (index == 0 || !Self::contains(value, index - 1, 1, vec!["E", "I"]))
                && (index == 0 || !Self::contains(value, index - 1, 3, vec!["RGY", "OGY"]))
        {
            //-- -ger-, -gy- --//
            //-- -ges-, -gep-, -gel-, -gie- at beginning --//
            result.append_char('K', Some('J'));
            1
        } else if Self::contains(value, index + 1, 1, vec!["E", "I", "Y"])
            || (index > 0 && Self::contains(value, index - 1, 4, vec!["AGGI", "OGGI"]))
        {
            //-- Italian "biaggi" --//
            if Self::contains(value, 0, 4, vec!["VAN ", "VON "])
                || Self::contains(value, 0, 3, vec!["SCH"])
                || Self::contains(value, index + 1, 2, vec!["ET"])
            {
                //-- obvious germanic --//
                result.append_char('K', None);
            } else if Self::contains(value, index + 1, 3, vec!["IER"]) {
                result.append_char('J', None);
            } else {
                result.append_char('J', Some('K'));
            }
            1
        } else if Self::char_at(value, index + 1) == Some('G') {
            result.append_char('K', None);
            1
        } else {
            result.append_char('K', None);
            0
        }
    }

    fn handle_gh(value: &str, result: &mut DoubleMetaphoneResult, index: isize) -> usize {
        // Unwrap is safe in the first if because index > 0
        if index > 0
            && !(is_vowel(
                Self::char_at(value, index - 1).map(|c| c.to_ascii_lowercase()),
                true,
            ))
        {
            result.append_char('K', None);
            1
        } else if index == 0 {
            if Self::char_at(value, index + 2) == Some('I') {
                result.append_char('J', None);
            } else {
                result.append_char('K', None);
            }
            1
        } else if (index > 1 && Self::contains(value, index - 2, 1, vec!["B", "H", "D"]))
            || (index > 2 && Self::contains(value, index - 3, 1, vec!["B", "H", "D"]))
            || (index > 3 && Self::contains(value, index - 4, 1, vec!["B", "H"]))
        {
            //-- Parker's rule (with some further refinements) - "hugh"
            1
        } else {
            if index > 2
                && Self::char_at(value, index - 1) == Some('U')
                && Self::contains(value, index - 3, 1, vec!["C", "G", "L", "R", "T"])
            {
                //-- "laugh", "McLaughlin", "cough", "gough", "rough", "tough"
                result.append_char('F', None);
            } else if index > 0 && Self::char_at(value, index - 1) != Some('I') {
                result.append_char('K', None);
            }
            1
        }
    }

    fn handle_h(value: &str, result: &mut DoubleMetaphoneResult, index: isize) -> usize {
        //-- only keep if first & before vowel or between 2 vowels --//
        if (index == 0
            || is_vowel(
                Self::char_at(value, index - 1).map(|c| c.to_ascii_lowercase()),
                true,
            ))
            && is_vowel(
                Self::char_at(value, index + 1).map(|c| c.to_ascii_lowercase()),
                true,
            )
        {
            result.append_char('H', None);
            1
            //-- also takes car of "HH" --//
        } else {
            0
        }
    }

    fn handle_j(
        value: &str,
        result: &mut DoubleMetaphoneResult,
        index: isize,
        slavo_germanic: bool,
    ) -> usize {
        if Self::contains(value, index, 4, vec!["JOSE"])
            || Self::contains(value, 0, 4, vec!["SAN "])
        {
            //-- obvious Spanish, "Jose", "San Jacinto" --//
            if (index == 0 && (Self::char_at(value, index + 4) == Some(' ')) || value.len() == 4)
                || Self::contains(value, 0, 4, vec!["SAN "])
            {
                result.append_char('H', None);
            } else {
                result.append_char('J', Some('H'));
            }
            0
        } else {
            if index == 0 && !Self::contains(value, index, 4, vec!["JOSE"]) {
                result.append_char('J', Some('A'));
            } else if index > 0
                && is_vowel(
                    Self::char_at(value, index - 1).map(|c| c.to_ascii_lowercase()),
                    true,
                )
                && !slavo_germanic
                && (Self::char_at(value, index + 1) == Some('A')
                    || Self::char_at(value, index + 1) == Some('O'))
            {
                result.append_char('J', Some('H'));
            } else if (index as usize) == value.len() - 1 {
                result.append_char('J', Some(' '));
            } else if !Self::contains_array(value, index + 1, 1, L_T_K_S_N_M_B_Z)
                && (index == 0 || !Self::contains(value, index - 1, 1, vec!["S", "K", "L"]))
            {
                result.append_char('J', None);
            }

            if Self::char_at(value, index + 1) == Some('J') {
                1
            } else {
                0
            }
        }
    }

    fn handle_l(value: &str, result: &mut DoubleMetaphoneResult, index: isize) -> usize {
        if Self::char_at(value, index + 1) == Some('L') {
            if Self::condition_l0(value, index) {
                result.append_char_primary('L');
            } else {
                result.append_char('L', None);
            }
            1
        } else {
            result.append_char('L', None);
            0
        }
    }

    fn condition_l0(value: &str, index: isize) -> bool {
        if value.len() >= 3
            && (index as usize) == value.len() - 3
            && index > 0
            && Self::contains(value, index - 1, 4, vec!["ILLO", "ILLA", "ALLE"])
        {
            return true;
        }

        ((value.len() > 1 && Self::contains(value, value.len() as isize - 2, 2, vec!["AS", "OS"]))
            || (!value.is_empty()
                && Self::contains(value, value.len() as isize - 1, 1, vec!["A", "O"])))
            && !value.is_empty()
            && Self::contains(value, index - 1, 4, vec!["ALLE"])
    }

    fn condition_m0(value: &str, index: isize) -> bool {
        if Self::char_at(value, index + 1) == Some('M') {
            return true;
        }

        index > 0
            && Self::contains(value, index - 1, 3, vec!["UMB"])
            && ((index + 1) == value.len() as isize - 1
                || Self::contains(value, index + 2, 2, vec!["ER"]))
    }

    fn handle_p(value: &str, result: &mut DoubleMetaphoneResult, index: isize) -> usize {
        if Self::char_at(value, index + 1) == Some('H') {
            result.append_char('F', None);
            1
        } else {
            result.append_char('P', None);
            if Self::contains(value, index + 1, 1, vec!["P", "B"]) {
                1
            } else {
                0
            }
        }
    }

    fn handle_r(
        value: &str,
        result: &mut DoubleMetaphoneResult,
        index: isize,
        slavo_germanic: bool,
    ) -> usize {
        if index > 3
            && index == value.len() as isize - 1
            && !slavo_germanic
            && Self::contains(value, index - 2, 2, vec!["IE"])
            && !Self::contains(value, index - 4, 2, vec!["ME", "MA"])
        {
            result.append_char_alternate('R');
        } else {
            result.append_char('R', None);
        }
        if Self::char_at(value, index + 1) == Some('R') {
            1
        } else {
            0
        }
    }

    fn handle_s(
        value: &str,
        result: &mut DoubleMetaphoneResult,
        index: isize,
        slavo_germanic: bool,
    ) -> usize {
        if index > 0 && Self::contains(value, index - 1, 3, vec!["ISL", "YSL"]) {
            //-- special cases "island", "isle", "carlisle", "carlysle" --//
            0
        } else if index == 0 && Self::contains(value, index, 5, vec!["SUGAR"]) {
            //-- special case "sugar-" --//
            result.append_char('X', Some('S'));
            0
        } else if Self::contains(value, index, 2, vec!["SH"]) {
            if Self::contains(value, index + 1, 4, vec!["HEIM", "HOEK", "HOLM", "HOLZ"]) {
                //-- germanic --//
                result.append_char('S', None);
            } else {
                result.append_char('X', None);
            }
            1
        } else if Self::contains(value, index, 3, vec!["SIO", "SIA"])
            || Self::contains(value, index, 4, vec!["SIAN"])
        {
            //-- Italian and Armenian --//
            if slavo_germanic {
                result.append_char('S', None);
            } else {
                result.append_char('S', Some('X'));
            }
            2
        } else if (index == 0 && Self::contains(value, index + 1, 1, vec!["M", "N", "L", "W"]))
            || Self::contains(value, index + 1, 1, vec!["Z"])
        {
            //-- german & anglicisations, e.g. "smith" match "schmidt" //
            // "snider" match "schneider" --//
            //-- also, -sz- in slavic language, although in hungarian it //
            //   is pronounced "s" --//
            result.append_char('S', Some('X'));
            if Self::contains(value, index + 1, 1, vec!["Z"]) {
                1
            } else {
                0
            }
        } else if Self::contains(value, index, 2, vec!["SC"]) {
            Self::handle_sc(value, result, index)
        } else {
            if index > 1
                && index == value.len() as isize - 1
                && Self::contains(value, index - 2, 2, vec!["AI", "OI"])
            {
                //-- french e.g. "resnais", "artois" --//
                result.append_char_alternate('S');
            } else {
                result.append_char('S', None);
            }
            if Self::contains(value, index + 1, 1, vec!["S", "Z"]) {
                1
            } else {
                0
            }
        }
    }

    fn handle_sc(value: &str, result: &mut DoubleMetaphoneResult, index: isize) -> usize {
        if Self::char_at(value, index + 2) == Some('H') {
            //-- Schlesinger's rule --//
            if Self::contains(
                value,
                index + 3,
                2,
                vec!["OO", "ER", "EN", "UY", "ED", "EM"],
            ) {
                //-- Dutch origin, e.g. "school", "schooner" --//
                if Self::contains(value, index + 3, 2, vec!["ER", "EN"]) {
                    //-- "schermerhorn", "schenker" --//
                    result.append_str("X", Some("SK"));
                } else {
                    result.append_str("SK", None);
                }
            } else if index == 0
                && !is_vowel(
                    Self::char_at(value, 3).map(|c| c.to_ascii_lowercase()),
                    true,
                )
                && Self::char_at(value, 3) != Some('W')
            {
                result.append_char('X', Some('S'));
            } else {
                result.append_char('X', None);
            }
        } else if Self::contains(value, index + 2, 1, vec!["I", "E", "Y"]) {
            result.append_char('S', None);
        } else {
            result.append_str("SK", None);
        }
        2
    }

    fn handle_t(value: &str, result: &mut DoubleMetaphoneResult, index: isize) -> usize {
        if Self::contains(value, index, 4, vec!["TION"])
            || Self::contains(value, index, 3, vec!["TIA", "TCH"])
        {
            result.append_char('X', None);
            2
        } else if Self::contains(value, index, 2, vec!["TH"])
            || Self::contains(value, index, 3, vec!["TTH"])
        {
            if Self::contains(value, index + 2, 2, vec!["OM", "AM"]) ||
                //-- special case "thomas", "thames" or germanic --//
                Self::contains(value, 0, 4, vec!["VAN ", "VON "]) ||
                Self::contains(value, 0, 3, vec!["SCH"])
            {
                result.append_char('T', None);
            } else {
                result.append_char('0', Some('T'));
            }
            1
        } else {
            result.append_char('T', None);
            if Self::contains(value, index + 1, 1, vec!["T", "D"]) {
                1
            } else {
                0
            }
        }
    }

    fn handle_w(value: &str, result: &mut DoubleMetaphoneResult, index: isize) -> usize {
        if Self::contains(value, index, 2, vec!["WR"]) {
            //-- can also be in middle of word --//
            result.append_char('R', None);
            1
        } else if index == 0
            && (is_vowel(
                Self::char_at(value, index + 1).map(|c| c.to_ascii_lowercase()),
                true,
            ) || Self::contains(value, index, 2, vec!["WH"]))
        {
            if is_vowel(
                Self::char_at(value, index + 1).map(|c| c.to_ascii_lowercase()),
                true,
            ) {
                //-- Wasserman should match Vasserman --//
                result.append_char('A', Some('F'));
            } else {
                //-- need Uomo to match Womo --//
                result.append_char('A', None);
            }
            0
        } else if (index > 0
            && index == value.len() as isize - 1
            && is_vowel(
                Self::char_at(value, index - 1).map(|c| c.to_ascii_lowercase()),
                true,
            ))
            || (index > 0
                && Self::contains(
                    value,
                    index - 1,
                    5,
                    vec!["EWSKI", "EWSKY", "OWSKI", "OWSKY"],
                ))
            || Self::contains(value, 0, 3, vec!["SCH"])
        {
            //-- Arnow should match Arnoff --//
            result.append_char_alternate('F');
            0
        } else if Self::contains(value, index, 4, vec!["WICZ", "WITZ"]) {
            //-- Polish e.g. "filipowicz" --//
            result.append_str("TS", Some("FX"));
            3
        } else {
            0
        }
    }

    fn handle_x(value: &str, result: &mut DoubleMetaphoneResult, index: isize) -> usize {
        if index == 0 {
            result.append_char('S', None);
            0
        } else {
            if !((index == value.len() as isize - 1)
                && ((index > 2 && Self::contains(value, index - 3, 3, vec!["IAU", "EAU"]))
                    || (index > 1 && Self::contains(value, index - 2, 2, vec!["AU", "OU"]))))
            {
                //-- French e.g. breaux --//
                result.append_str("KS", None);
            }
            if Self::contains(value, index + 1, 1, vec!["C", "X"]) {
                1
            } else {
                0
            }
        }
    }

    fn handle_z(
        value: &str,
        result: &mut DoubleMetaphoneResult,
        index: isize,
        slavo_germanic: bool,
    ) -> usize {
        if Self::char_at(value, index + 1) == Some('H') {
            //-- Chinese pinyin e.g. "zhao" or Angelina "Zhang" --//
            result.append_char('J', None);
            1
        } else {
            if Self::contains(value, index + 1, 2, vec!["ZO", "ZI", "ZA"])
                || (slavo_germanic && (index > 0 && Self::char_at(value, index - 1) != Some('T')))
            {
                result.append_str("S", Some("TS"));
            } else {
                result.append_char('S', None);
            }
            if Self::char_at(value, index + 1) == Some('Z') {
                1
            } else {
                0
            }
        }
    }
}

