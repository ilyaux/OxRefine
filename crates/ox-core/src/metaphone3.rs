// Vendored from the metaphone3 crate 0.2.0 (src/lib.rs, MIT License,
// https://github.com/kakserpom/metaphone3-rs), a port of Lawrence Philips' Metaphone 3.
// Changed for OxRefine: vowels with diacritics count as vowels, as in the Metaphone 3 OpenRefine
// runs (the crate only knew AEIOUY: "Ørsted" lost its leading A, "Jürgen" read as German "-RGEN");
// std String instead of smartstring; the test module is not included.
//! A pure-Rust implementation of the **Metaphone 3** phonetic encoding algorithm.
//!
//! Metaphone 3 maps words that sound alike in American English onto the same
//! keys, which makes it useful for fuzzy matching, name search, and phonetic
//! comparison. Each word encodes to a *primary* key and, when the pronunciation
//! is ambiguous, a *secondary* (alternate) key. Both keys are at most 8
//! characters long.
//!
//! # Example
//!
//! ```
//! use ox_core::metaphone3::Metaphone3;
//!
//! let mut encoder = Metaphone3::new();
//! let (primary, secondary) = encoder.encode("Smith");
//! assert_eq!(primary, "SM0");
//! assert_eq!(secondary, "XMT");
//! ```
//!
//! The encoder is reusable across calls to minimize allocations, and supports
//! optional [vowel](Metaphone3::with_encode_vowels) and
//! [exact](Metaphone3::with_encode_exact) encoding modes via a builder-style API.
//!
//! [`Metaphone3`] is **not** thread-safe; use one encoder per thread.

// Rust port of the Metaphone3 algorithm.
// These pedantic lints are intentionally allowed: the algorithm is a faithful
// port whose index arithmetic relies on `usize`/`isize` casts that are correct
// by construction (indices stay within short words), whose boolean conditionals
// and short binding names deliberately mirror the reference implementation for
// auditability, and whose per-letter encoding routines are long by nature.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::nonminimal_bool,
    clippy::if_not_else,
    clippy::too_many_lines
)]


const METAPH_MAX_LENGTH: usize = 8;


/// A Metaphone 3 encoder.
///
/// Construct one with [`Metaphone3::new`], optionally configure it with
/// [`with_encode_vowels`](Metaphone3::with_encode_vowels) /
/// [`with_encode_exact`](Metaphone3::with_encode_exact), then call
/// [`encode`](Metaphone3::encode). A single instance can encode many words and
/// reuses its internal buffers between calls.
pub struct Metaphone3 {
    in_buf: Vec<char>,
    length: usize,
    idx: usize,
    last_idx: usize,
    prim_buf: Vec<char>,
    second_buf: Vec<char>,
    encode_vowels: bool,
    encode_exact: bool,
    flag_al_inversion: bool,
}

impl Metaphone3 {
    /// Creates a new Metaphone3 encoder with default settings.
    #[must_use]
    pub fn new() -> Self {
        Metaphone3 {
            in_buf: Vec::new(),
            length: 0,
            idx: 0,
            last_idx: 0,
            prim_buf: Vec::new(),
            second_buf: Vec::new(),
            encode_vowels: false,
            encode_exact: false,
            flag_al_inversion: false,
        }
    }

    /// Sets the option to encode vowels.
    #[must_use]
    pub fn with_encode_vowels(mut self, encode: bool) -> Self {
        self.encode_vowels = encode;
        self
    }

    /// Sets the option for more exact encoding.
    #[must_use]
    pub fn with_encode_exact(mut self, encode: bool) -> Self {
        self.encode_exact = encode;
        self
    }

    /// Encodes a word into its primary and secondary Metaphone 3 keys.
    ///
    /// Returns a `(primary, secondary)` tuple. The primary key is always present
    /// for non-empty input; the secondary key is empty when the word has no
    /// alternate pronunciation. Both keys are at most 8 characters long. Empty
    /// input yields two empty strings.
    ///
    /// # Example
    ///
    /// ```
    /// # use ox_core::metaphone3::Metaphone3;
    /// let mut encoder = Metaphone3::new();
    /// let (primary, secondary) = encoder.encode("Aachen");
    /// assert_eq!(primary, "AKN");
    /// assert_eq!(secondary, "AXN");
    /// ```
    pub fn encode(&mut self, word: &str) -> (String, String) {
        if word.is_empty() {
            return (String::new(), String::new());
        }

        // Reset state
        self.flag_al_inversion = false;
        // Uppercase char-by-char, reusing the existing buffer's capacity to avoid
        // allocating a temporary String plus a fresh Vec on every call.
        self.in_buf.clear();
        self.in_buf
            .extend(word.chars().flat_map(char::to_uppercase));
        self.length = self.in_buf.len();
        self.last_idx = self.length - 1;

        // Prepare output buffers
        self.prim_buf.clear();
        self.prim_buf.reserve(METAPH_MAX_LENGTH);
        self.second_buf.clear();
        self.second_buf.reserve(METAPH_MAX_LENGTH);

        // Main encoding loop - rune by rune through the input
        self.idx = 0;
        while self.idx < self.length {
            // Check if buffers are full
            if self.prim_buf.len() >= METAPH_MAX_LENGTH
                && self.second_buf.len() >= METAPH_MAX_LENGTH
            {
                break;
            }

            let c = self.in_buf[self.idx];

            match c {
                'B' => self.encode_b(),
                'ß' | 'Ç' => self.metaph_add('S'),
                'C' => self.encode_c(),
                'D' => self.encode_d(),
                'F' => self.encode_f(),
                'G' => self.encode_g(),
                'H' => self.encode_h(),
                'J' => self.encode_j(),
                'K' => self.encode_k(),
                'L' => self.encode_l(),
                'M' => self.encode_m(),
                'N' => self.encode_n(),
                'Ñ' => self.metaph_add('N'),
                'P' => self.encode_p(),
                'Q' => self.encode_q(),
                'R' => self.encode_r(),
                'S' => self.encode_s(),
                'T' => self.encode_t(),
                'Ð' | 'Þ' => self.metaph_add('0'),
                'V' => self.encode_v(),
                'W' => self.encode_w(),
                'X' => self.encode_x(),
                'Z' => self.encode_z(),
                _ => {
                    if Self::is_vowel_char(c) {
                        self.encode_vowels();
                    }
                }
            }

            // Always increment idx to match Go's for loop behavior (e.idx++)
            // This happens regardless of whether the encoder modified idx
            self.idx += 1;
        }

        // Trim buffers if needed
        if self.prim_buf.len() > METAPH_MAX_LENGTH {
            self.prim_buf.truncate(METAPH_MAX_LENGTH);
        }
        if self.second_buf.len() > METAPH_MAX_LENGTH {
            self.second_buf.truncate(METAPH_MAX_LENGTH);
        }

        // Convert to strings
        let primary: String = self.prim_buf.iter().copied().collect();
        let secondary: String = self.second_buf.iter().copied().collect();

        if primary == secondary {
            (primary, String::new())
        } else {
            (primary, secondary)
        }
    }

    // ==============================================================================================
    // Letter encoding methods (to be ported from Go)
    // ==============================================================================================

    fn encode_b(&mut self) {
        if self.encode_silent_b() {
            return;
        }

        // "-mb", e.g", "dumb", already skipped over under
        // 'M', altho it should really be handled here...
        self.metaph_add_exact_approx('B', 'P');

        // skip double B, or BPx where X isn't H
        if self.char_next_is('B')
            || (self.char_next_is('P')
                && self.idx + 2 < self.length
                && self.in_buf[self.idx + 2] != 'H')
        {
            self.idx += 1;
        }
    }

    /// Encodes silent 'B' for cases not covered under "-mb-"
    fn encode_silent_b(&mut self) -> bool {
        //'debt', 'doubt', 'subtle'
        if self.string_at(-2, &["DEBT", "SUBTL", "SUBTIL"]) || self.string_at(-3, &["DOUBT"]) {
            self.metaph_add('T');
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_c(&mut self) {
        if self.encode_silent_c_at_beginning()
            || self.encode_ca_to_s()
            || self.encode_co_to_s()
            || self.encode_ch()
            || self.encode_ccia()
            || self.encode_cc()
            || self.encode_ck_cg_cq()
            || self.encode_c_front_vowel()
            || self.encode_silent_c()
            || self.encode_cz()
            || self.encode_cs()
        {
            return;
        }

        if !self.string_at(-1, &["C", "K", "G", "Q"]) {
            self.metaph_add('K');
        }

        //name sent in 'mac caffrey', 'mac gregor
        if self.string_at(1, &[" C", " Q", " G"]) {
            self.idx += 1;
        } else if self.string_at(1, &["C", "K", "Q"]) && !self.string_at(1, &["CE", "CI"]) {
            self.idx += 1; // increment 1 here, so adjust offsets below
            // account for combinations such as Ro-ckc-liffe
            if self.string_at(1, &["C", "K", "Q"]) && !self.string_at(2, &["CE", "CI"]) {
                self.idx += 1;
            }
        }
    }

    fn encode_silent_c_at_beginning(&mut self) -> bool {
        if self.idx == 0 && self.string_at(0, &["CT", "CN"]) {
            return true;
        }
        false
    }

    //Encodes exceptions where "-CA-" should encode to S
    //instead of K including cases where the cedilla has not been used
    fn encode_ca_to_s(&mut self) -> bool {
        // Special case: 'caesar'.
        // Also, where cedilla not used, as in "linguica" => LNKS
        if (self.idx == 0 && self.string_at(0, &["CAES", "CAEC", "CAEM"]))
            || self.string_start(&[
                "FACADE",
                "FRANCAIS",
                "FRANCAIX",
                "LINGUICA",
                "GONCALVES",
                "PROVENCAL",
            ])
        {
            self.metaph_add('S');
            self.advance_counter(1, 0);
            return true;
        }

        false
    }

    //Encodes exceptions where "-CO-" encodes to S instead of K
    //including cases where the cedilla has not been used
    fn encode_co_to_s(&mut self) -> bool {
        // e.g. 'coelecanth' => SLKN0
        if (self.string_at(0, &["COEL"]) && (self.is_vowel_at(4) || self.idx + 3 == self.last_idx))
            || self.string_at(0, &["COENA", "COENO"])
            || self.string_start(&["GARCON", "FRANCOIS", "MELANCON"])
        {
            self.metaph_add('S');
            self.advance_counter(2, 0);
            return true;
        }

        false
    }

    fn encode_ch(&mut self) -> bool {
        if !self.string_at(0, &["CH"]) {
            return false;
        }

        if self.encode_chae()
            || self.encode_ch_to_h()
            || self.encode_silent_ch()
            || self.encode_arch()
            || self.encode_ch_to_x()
            || self.encode_english_ch_to_k()
            || self.encode_germanic_ch_to_k()
            || self.encode_greek_ch_initial()
            || self.encode_greek_ch_non_initial()
        {
            return true;
        }

        if self.idx > 0 {
            if self.string_start(&["MC"]) && self.idx == 1 {
                //e.g., "McHugh"
                self.metaph_add('K');
            } else {
                self.metaph_add_alt('X', 'K');
            }
        } else {
            self.metaph_add('X');
        }

        self.idx += 1;
        true
    }

    fn encode_chae(&mut self) -> bool {
        // e.g. 'michael'
        if self.idx > 0 && self.string_at(2, &["AE"]) {
            if self.string_start(&["RACHAEL"]) {
                self.metaph_add('X');
            } else if !self.string_at(-1, &["C", "K", "G", "Q"]) {
                self.metaph_add('K');
            }

            self.advance_counter(3, 1);
            return true;
        }

        false
    }

    // Encodes transliterations from the hebrew where the
    // sound 'kh' is represented as "-CH-". The normal pronounciation
    // of this in english is either 'h' or 'kh', and alternate
    // spellings most often use "-H-"
    fn encode_ch_to_h(&mut self) -> bool {
        // hebrew => 'H', e.g. 'channukah', 'chabad'
        if (self.idx == 0
            && (self.string_at(
                2,
                &[
                    "AIM", "ETH", "ELM", "ASID", "AZAN", "UPPAH", "UTZPA", "ALLAH", "ALUTZ",
                    "AMETZ", "ESHVAN", "ADARIM", "ANUKAH", "ALLLOTH", "ANNUKAH", "AROSETH",
                ],
            )))
            || self.string_at(-3, &["CLACHAN"])
        {
            self.metaph_add('H');
            self.advance_counter(2, 1);
            return true;
        }

        false
    }

    fn encode_silent_ch(&mut self) -> bool {
        if self.string_at(-2, &["YACHT", "FUCHSIA"])
            || self.string_start(&["STRACHAN", "CRICHTON"])
            || (self.string_at(-3, &["DRACHM"]) && !self.string_at(-3, &["DRACHMA"]))
        {
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_ch_to_x(&mut self) -> bool {
        // e.g. 'approach', 'beach'
        if (self.string_at(-2, &["OACH", "EACH", "EECH", "OUCH", "OOCH", "MUCH", "SUCH"])
            && !self.string_at(-3, &["JOACH"]))
            || self.string_at_end(-1, &["ACHA", "ACHO"]) // e.g. 'dacha', 'macho'
            || self.string_at_end(0, &["CHOT", "CHOD", "CHAT"])
            || (self.string_at_end(-1, &["OCHE"]) && !self.string_at(-2, &["DOCHE"]))
            || self.string_at(-4, &["ATTACH", "DETACH", "KOVACH", "PARACHUT"])
            || self.string_at(-5, &["SPINACH", "MASSACHU"])
            || self.string_start(&["MACHAU"])
            || (self.string_at(-3, &["THACH"]) && !self.string_at(2, &["E"])) // no ACHE
            || self.string_at(-2, &["VACHON"])
        {
            self.metaph_add('X');
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_english_ch_to_k(&mut self) -> bool {
        //'ache', 'echo', alternate spelling of 'michael'
        if (self.idx == 1 && self.root_or_inflections("ACHE"))
            || ((self.idx > 3 && self.root_or_inflections_from(self.idx - 1, "ACHE"))
                && self.string_start(&["EAR", "HEAD", "BACK", "HEART", "BELLY", "TOOTH"]))
            || self.string_at(-1, &["ECHO"])
            || self.string_at(-2, &["MICHEAL"])
            || self.string_at(-4, &["JERICHO"])
            || self.string_at(-5, &["LEPRECH"])
        {
            self.metaph_add_alt('K', 'X');
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_germanic_ch_to_k(&mut self) -> bool {
        // various germanic
        // "<consonant><vowel>CH-"implies a german word where 'ch' => K

        if (self.idx > 1
            && !self.is_vowel_at(-2)
            && self.string_at(-1, &["ACH"])
            && !self.string_at(-2, &["MACHADO", "MACHUCA", "LACHANC", "LACHAPE", "KACHATU"])
            && !self.string_at(-3, &["KHACHAT"])
            && (!self.char_at(2, 'I')
                && (!self.char_at(2, 'E')
                    || self.string_at(-2, &["BACHER", "MACHER", "MACHEN", "LACHER"])))
            || // e.g. 'brecht', 'fuchs'
            (self.string_at(2, &["T", "S"]) && !self.string_start(&["LUNCHTIME", "WHICHSOEVER"]))
            || // e.g. 'andromache'
            self.string_start(&["SCHR"])
            || (self.idx > 2 && self.string_at(-2, &["MACHE"]))
            || (self.idx == 2 && self.string_at(-2, &["ZACH"]))
            || self.string_at(-4, &["SCHACH"])
            || self.string_at(-1, &["ACHEN"])
            || self.string_at(-3, &["SPICH", "ZURCH", "BUECH"])
            || (self.string_at(-3, &["KIRCH", "JOACH", "BLECH", "MALCH"])
                && !(self.string_at(-3, &["KIRCHNER"]) || self.idx + 1 == self.last_idx)) // "kirch" and "blech" both get 'X'
            || self.string_at_end(-2, &["NICH", "LICH", "BACH"])
            || (self.string_at_end(-3, &["URICH", "BRICH", "ERICH", "DRICH", "NRICH"])
                && !self.string_at_end(-5, &["ALDRICH"])
                && !self.string_at_end(-6, &["GOODRICH"])
                && !self.string_at_end(-7, &["GINGERICH"])))
            || self.string_at_end(-4, &["ULRICH", "LFRICH", "LLRICH", "EMRICH", "ZURICH", "EYRICH"])
            || // e.g., 'wachtler', 'wechsler', but not 'tichner'
            ((self.string_at(-1, &["A", "O", "U", "E"]) || self.idx == 0)
                && self.string_at(2, &["L", "R", "N", "M", "B", "H", "F", "V", "W", " "]))
        {
            // "CHR/L-" e.g. 'chris' do not get
            // alt pronunciation of 'X'
            if self.string_at(2, &["R", "L"]) || self.is_slavo_germanic() {
                self.metaph_add('K');
            } else {
                self.metaph_add_alt('K', 'X');
            }
            self.idx += 1;
            return true;
        }

        false
    }

    // Encode "-ARCH-". Some occurances are from greek roots and therefore encode
    // to 'K', others are from english words and therefore encode to 'X'
    fn encode_arch(&mut self) -> bool {
        if self.string_at(-2, &["ARCH"]) {
            // "-ARCH-" has many combining forms where "-CH-" => K because of its
            // derivation from the greek
            if ((self.is_vowel_at(2)
                && self.string_at(-2, &["ARCHA", "ARCHI", "ARCHO", "ARCHU", "ARCHY"]))
                || self.string_at(
                    -2,
                    &[
                        "ARCHEA", "ARCHEG", "ARCHEO", "ARCHET", "ARCHEL", "ARCHES", "ARCHEP",
                        "ARCHEM", "ARCHEN",
                    ],
                )
                || self.string_at_end(-2, &["ARCH"])
                || self.string_start(&["MENARCH"]))
                && (!self.root_or_inflections("ARCH")
                    && !self.string_at(-4, &["SEARCH", "POARCH"])
                    && !self.string_start(&[
                        "ARCHER",
                        "ARCHIE",
                        "ARCHENEMY",
                        "ARCHIBALD",
                        "ARCHULETA",
                        "ARCHAMBAU",
                    ])
                    && !((((self.string_at(-3, &["LARCH", "MARCH", "PARCH"])
                        || self.string_at(-4, &["STARCH"]))
                        && !self.string_start(&[
                            "EPARCH",
                            "NOMARCH",
                            "EXILARCH",
                            "HIPPARCH",
                            "MARCHESE",
                            "ARISTARCH",
                            "MARCHETTI",
                        ]))
                        || self.root_or_inflections("STARCH"))
                        && (!self.string_at(-2, &["ARCHU", "ARCHY"])
                            || self.string_start(&["STARCHY"]))))
            {
                self.metaph_add_alt('K', 'X');
            } else {
                self.metaph_add('X');
            }
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_greek_ch_initial(&mut self) -> bool {
        // greek roots e.g. 'chemistry', 'chorus', ch at beginning of root
        if (self.string_at(
            0,
            &[
                "CHAMOM", "CHARAC", "CHARIS", "CHARTO", "CHARTU", "CHARYB", "CHRIST", "CHEMIC",
                "CHILIA",
            ],
        ) || (self.string_at(
            0,
            &[
                "CHEMI", "CHEMO", "CHEMU", "CHEMY", "CHOND", "CHONA", "CHONI", "CHOIR", "CHASM",
                "CHARO", "CHROM", "CHROI", "CHAMA", "CHALC", "CHALD", "CHAET", "CHIRO", "CHILO",
                "CHELA", "CHOUS", "CHEIL", "CHEIR", "CHEIM", "CHITI", "CHEOP",
            ],
        ) && !(self.string_at(0, &["CHEMIN"]) || self.string_at(-2, &["ANCHONDO"])))
            || (self.string_at(0, &["CHISM", "CHELI"])
                && // exclude spanish "machismo"
                !(self.string_start(&["MICHEL", "MACHISMO", "RICHELIEU", "REVANCHISM"])
                    || self.string_exact(&["CHISM"])))
            || // include e.g. "chorus", "chyme", "chaos"
            (self.string_at(0, &["CHOR", "CHOL", "CHYM", "CHYL", "CHLO", "CHOS", "CHUS", "CHOE"])
                && !self.string_start(&["CHOLLO", "CHOLLA", "CHORIZ"]))
            || // "chaos" => K but not "chao"
            (self.string_at(0, &["CHAO"]) && self.idx + 3 != self.last_idx)
            || // e.g. "abranchiate"
            (self.string_at(0, &["CHIA"]) && !self.string_start(&["CHIAPAS", "APPALACHIA"]))
            || // e.g. "chimera"
            self.string_at(0, &["CHIMERA", "CHIMAER", "CHIMERI"])
            || // e.g. "chameleon"
            self.string_start(&["CHAME", "CHELO", "CHITO"])
            || // e.g. "spirochete"
            ((self.idx + 4 == self.last_idx || self.idx + 5 == self.last_idx)
                && self.string_at(-1, &["OCHETE"])))
            && // more exceptions where "-CH-" => X e.g. "chortle", "crocheter"
            !(self.string_exact(&["CHORE", "CHOLO", "CHOLA"])
                || self.string_at(0, &["CHORT", "CHOSE"])
                || self.string_at(-3, &["CROCHET"])
                || self.string_start(&["CHEMISE", "CHARISE", "CHARISS", "CHAROLE"]))
        {
            if self.string_at(2, &["R", "L"]) {
                self.metaph_add('K');
            } else {
                self.metaph_add_alt('K', 'X');
            }
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_greek_ch_non_initial(&mut self) -> bool {
        //greek & other roots e.g. 'tachometer', 'orchid', ch in middle or end of root
        if self.string_at(
            -2,
            &[
                "LYCHN", "TACHO", "ORCHO", "ORCHI", "LICHO", "ORCHID", "NICHOL", "MECHAN",
                "LICHEN", "MACHIC", "PACHEL", "RACHIF", "RACHID", "RACHIS", "RACHIC", "MICHAL",
                "ORCHESTR",
            ],
        ) || self.string_at(
            -3,
            &[
                "MELCH", "GLOCH", "TRACH", "TROCH", "BRACH", "SYNCH", "PSYCH", "STICH", "PULCH",
                "EPOCH",
            ],
        ) || (self.string_at(-3, &["TRICH"]) && !self.string_at(-5, &["OSTRICH"]))
            || (self.string_at(
                -2,
                &[
                    "TYCH", "TOCH", "BUCH", "MOCH", "CICH", "DICH", "NUCH", "EICH", "LOCH",
                    "DOCH", "ZECH", "WYCH",
                ],
            ) && !(self.string_at(-4, &["INDOCHINA"]) || self.string_at(-2, &["BUCHON"])))
            || ((self.idx == 1 || self.idx == 2)
                && self.string_at(-1, &["OCHER", "ECHIN", "ECHID"]))
            || self.string_at(
                -4,
                &[
                    "BRONCH", "STOICH", "STRYCH", "TELECH", "PLANCH", "CATECH", "MANICH",
                    "MALACH", "BIANCH", "DIDACH", "BRANCHIO", "BRANCHIF",
                ],
            )
            || self.string_start(&["ICHA", "ICHN"])
            || (self.string_at(-1, &["ACHAB", "ACHAD", "ACHAN", "ACHAZ"])
                && !self.string_at(-2, &["MACHADO", "LACHANC"]))
            || self.string_at(
                -1,
                &[
                    "ACHISH", "ACHILL", "ACHAIA", "ACHENE", "ACHAIAN", "ACHATES", "ACHIRAL",
                    "ACHERON", "ACHILLEA", "ACHIMAAS", "ACHILARY", "ACHELOUS", "ACHENIAL",
                    "ACHERNAR", "ACHALASIA", "ACHILLEAN", "ACHIMENES", "ACHIMELECH",
                    "ACHITOPHEL",
                ],
            )
            || // e.g. 'inchoate'
            (self.idx == 2 && (self.string_start(&["INCHOA"])
            // e.g. 'ischemia'
            || self.string_start(&["ISCH"])))
            || // e.g. 'ablimelech', 'antioch', 'pentateuch'
            (self.idx + 1 == self.last_idx
                && self.string_at(-1, &["A", "O", "U", "E"])
                && !(self.string_start(&["DEBAUCH"])
                    || self.string_at(-2, &["MUCH", "SUCH", "KOCH"])
                    || self.string_at(-5, &["OODRICH", "ALDRICH"])))
        {
            self.metaph_add_alt('K', 'X');
            self.idx += 1;
            return true;
        }

        false
    }

    //Encodes reliably italian "-CCIA-"
    fn encode_ccia(&mut self) -> bool {
        //e.g., 'focaccia'
        if self.string_at(1, &["CIA"]) {
            self.metaph_add_alt('X', 'S');
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_cc(&mut self) -> bool {
        //double 'C', but not if e.g. 'McClellan'
        if self.string_at(0, &["CC"]) && !(self.idx == 1 && self.in_buf[0] == 'M') {
            // exception
            if self.string_at(-3, &["FLACCID"]) {
                self.metaph_add('S');
                self.advance_counter(2, 1);
                return true;
            }

            //'bacci', 'bertucci', other italian
            if self.string_at_end(2, &["I"])
                || self.string_at(2, &["IO"])
                || self.string_at_end(2, &["INO", "INI"])
            {
                self.metaph_add('X');
                self.advance_counter(2, 1);
                return true;
            }

            //'accident', 'accede' 'succeed'
            if self.string_at(2, &["I", "E", "Y"]) &&//except 'bellocchio','bacchus', 'soccer' get K
            !(self.char_at(2, 'H') || self.string_at(-2, &["SOCCER"]))
            {
                self.metaph_add_str("KS", "KS");
                self.advance_counter(2, 1);
                return true;
            }
            // Pierce's rule
            self.metaph_add('K');
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_ck_cg_cq(&mut self) -> bool {
        if self.string_at(0, &["CK", "CG", "CQ"]) {
            // eastern european spelling e.g. 'gorecki' == 'goresky'
            if self.string_at_end(0, &["CKI", "CKY"]) && self.length > 6 {
                self.metaph_add_str("K", "SK");
            } else {
                self.metaph_add('K');
            }
            self.idx += 1; // skip the C
            // if there's a C[KGQ][KGQ] then skip that second one too
            if self.string_at(1, &["K", "G", "Q"]) {
                self.idx += 1;
            }

            return true;
        }

        false
    }

    //Encode cases where "C" preceeds a front vowel such as "E", "I", or "Y".
    //These cases most likely => S or X
    fn encode_c_front_vowel(&mut self) -> bool {
        if self.string_at(0, &["CI", "CE", "CY"]) {
            if self.encode_british_silent_ce()
                || self.encode_ce()
                || self.encode_ci()
                || self.encode_latinate_suffixes()
            {
                self.advance_counter(1, 0);
                return true;
            }

            self.metaph_add('S');
            self.advance_counter(1, 0);
            return true;
        }

        false
    }

    fn encode_british_silent_ce(&mut self) -> bool {
        // english place names like e.g.'gloucester' pronounced glo-ster
        if self.string_at_end(1, &["ESTER"]) || self.string_at(1, &["ESTERSHIRE"]) {
            return true;
        }

        false
    }

    fn encode_ce(&mut self) -> bool {
        // 'ocean', 'commercial', 'provincial', 'cello', 'fettucini', 'medici'
        if (self.string_at(1, &["EAN"]) && self.is_vowel_at(-1))
            || (self.string_at_end(-1, &["ACEA"]) && !self.string_start(&["PANACEA"])) // e.g. 'rosacea'
            || self.string_at(1, &["ELLI", "ERTO", "EORL"]) // e.g. 'botticelli', 'concerto'
            || self.string_at_end(-3, &["CROCE"]) // some italian names familiar to americans
            || self.string_at(-3, &["DOLCE"])
            || self.string_at_end(1, &["ELLO"])
        // e.g. cello
        {
            self.metaph_add_alt('X', 'S');
            return true;
        }

        false
    }

    fn encode_ci(&mut self) -> bool {
        // with consonant before C
        // e.g. 'fettucini', but exception for the americanized pronunciation of 'mancini'

        if (self.string_at_end(1, &["INI"]) && !self.string_exact(&["MANCINI"]))
            || self.string_at_end(-1, &["ICI"]) // e.g. 'medici'
            || self.string_at(-1, &["RCIAL", "NCIAL", "RCIAN", "UCIUS"]) // e.g. "commercial', 'provincial', 'cistercian'
            || self.string_at(-3, &["MARCIA"]) // special cases
            || self.string_at(-2, &["ANCIENT"])
        {
            self.metaph_add_alt('X', 'S');
            return true;
        }

        // exception
        if self.string_at(-4, &["COERCION"]) {
            self.metaph_add('J');
            return true;
        }

        // with vowel before C (or at beginning?)
        if (self.string_at(0, &["CIO", "CIE", "CIA"]) && self.is_vowel_at(-1))
            || self.string_at(1, &["IAO"])
        {
            if (self.string_at(
                0,
                &[
                    "CIAN", "CIAL", "CIAO", "CIES", "CIOL", "CION",
                ],
            ) || self.string_at(-3, &["GLACIER"]) // exception - "glacier" => 'X' but "spacier" = > 'S'
                || self.string_at(
                    0,
                    &[
                        "CIENT", "CIENC", "CIOUS", "CIATE", "CIATI", "CIATO", "CIABL", "CIARY",
                    ],
                )
                || self.string_at_end(0, &["CIA", "CIO", "CIAS", "CIOS"]))
                && !(self.string_at(-4, &["ASSOCIATION"])
                    || self.string_start(&["OCIE"])
                    || // exceptions mostly because these names are usually from
                    // the spanish rather than the italian in america
                    self.string_at(-2, &["LUCIO", "SOCIO", "SOCIE", "MACIAS", "LUCIANO", "HACIENDA"])
                    || self.string_at(-3, &["GRACIE", "GRACIA", "MARCIANO"])
                    || self.string_at(-4, &["PALACIO", "POLICIES", "FELICIANO"])
                    || self.string_at(-5, &["MAURICIO"])
                    || self.string_at(-6, &["ANDALUCIA"])
                    || self.string_at(-7, &["ENCARNACION"]))
            {
                self.metaph_add_alt('X', 'S');
            } else {
                self.metaph_add_alt('S', 'X');
            }

            return true;
        }

        false
    }

    fn encode_latinate_suffixes(&mut self) -> bool {
        if self.string_at(1, &["EOUS", "IOUS"]) {
            self.metaph_add_alt('X', 'S');
            return true;
        }
        false
    }

    fn encode_silent_c(&mut self) -> bool {
        if self.string_at(1, &["T", "S"]) && self.string_start(&["INDICT", "TUCSON", "CONNECTICUT"])
        {
            return true;
        }

        false
    }

    // Encodes slavic spellings or transliterations
    // written as "-CZ-"
    fn encode_cz(&mut self) -> bool {
        if self.string_at(1, &["Z"]) && !self.string_at(-1, &["ECZEMA"]) {
            if self.string_at(0, &["CZAR"]) {
                self.metaph_add('S');
            } else {
                // otherwise most likely a czech word...
                self.metaph_add('X');
            }
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_cs(&mut self) -> bool {
        // give an 'etymological' 2nd
        // encoding for "kovacs" so
        // that it matches "kovach"

        if self.string_start(&["KOVACS"]) {
            self.metaph_add_str("KS", "X");
            self.idx += 1;
            return true;
        }

        if self.string_at_end(-1, &["ACS"]) && !self.string_at(-4, &["ISAACS"]) {
            self.metaph_add('X');
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_d(&mut self) {
        if self.encode_dg()
            || self.encode_dj()
            || self.encode_dt_dd()
            || self.encode_d_to_j()
            || self.encode_dous()
            || self.encode_silent_d()
        {
            return;
        }

        if self.encode_exact {
            // "final de-voicing" in this case
            // e.g. 'missed' == 'mist'
            if self.string_at_end(-3, &["SSED"]) {
                self.metaph_add('T');
            } else {
                self.metaph_add('D');
            }
        } else {
            self.metaph_add('T');
        }
        // Don't increment idx - let main loop handle it
    }

    fn encode_dg(&mut self) -> bool {
        if self.string_at(0, &["DG"]) {
            // excludes exceptions e.g. 'edgar',
            // or cases where 'g' is first letter of combining form
            if self.string_at(2, &["A", "O"])
                || self.string_at(
                    1,
                    &[
                        "GUN", "GUT", "GEAR", "GLAS", "GRIP", "GREN", "GILL", "GRAF", "GUARD",
                        "GUILT", "GRAVE", "GRASS", "GROUSE",
                    ],
                )
            {
                self.metaph_add_exact_approx_alt("DG", "DG", "TK", "TK");
            } else {
                // e.g. "edge", "abridgment"
                self.metaph_add('J');
            }

            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_dj(&mut self) -> bool {
        // e.g. "adjacent"
        if self.string_at(0, &["DJ"]) {
            self.metaph_add('J');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_dt_dd(&mut self) -> bool {
        // eat redundant 'T' or 'D'
        if self.string_at(0, &["DT", "DD"]) {
            if self.string_at(0, &["DTH"]) {
                self.metaph_add_exact_approx_alt("D0", "D0", "T0", "T0");
                self.idx += 2;
            } else {
                if self.encode_exact {
                    // devoice it
                    if self.string_at(0, &["DT"]) {
                        self.metaph_add('T');
                    } else {
                        self.metaph_add('D');
                    }
                } else {
                    self.metaph_add('T');
                }
                self.idx += 1;
            }

            return true;
        }
        false
    }

    fn encode_d_to_j(&mut self) -> bool {
        // e.g. "module", "adulate"
        if (self.string_at(0, &["DUL"]) && self.is_vowel_at(-1) && self.is_vowel_at(3)) ||
            // e.g. "soldier", "grandeur", "procedure"
            self.string_at_end(-1, &["LDIER", "NDEUR", "EDURE", "RDURE"]) ||
            self.string_at(-3, &["CORDIAL"]) ||
            // e.g. "pendulum", "education"
            self.string_at(-1, &["ADUA", "IDUA", "IDUU", "NDULA", "NDULU", "EDUCA"])
        {
            self.metaph_add_exact_approx_alt("J", "D", "J", "T");
            self.advance_counter(1, 0);
            return true;
        }
        false
    }

    fn encode_dous(&mut self) -> bool {
        // e.g. "assiduous", "arduous"
        if self.string_at(1, &["UOUS"]) {
            self.metaph_add_exact_approx_alt("J", "D", "J", "T");
            self.advance_counter(3, 0);
            return true;
        }
        false
    }

    fn encode_silent_d(&mut self) -> bool {
        // silent 'D' e.g. 'wednesday', 'handsome'
        self.string_at(-2, &["WEDNESDAY"]) ||
            self.string_at(-3, &["HANDKER", "HANDSOM", "WINDSOR"]) ||
            // french silent D at end in words or names familiar to americans
            self.string_end(&["PERNOD", "ARTAUD", "RENAUD", "RIMBAUD", "MICHAUD", "BICHAUD"])
    }

    fn encode_f(&mut self) {
        // Encode cases where "-FT-" => "T" is usually silent
        // e.g. 'often', 'soften'
        // This should really be covered under "T"!
        if self.string_at(-1, &["OFTEN"]) {
            self.metaph_add_str("F", "FT");
            self.idx += 1;
            return;
        }

        // eat redundant 'F'
        if self.char_next_is('F') {
            self.idx += 1;
        }
        self.metaph_add('F');
    }

    fn encode_g(&mut self) {
        if self.encode_silent_g_at_beginning()
            || self.encode_gg()
            || self.encode_gk()
            || self.encode_gh()
            || self.encode_silent_g()
            || self.encode_gn()
            || self.encode_gl()
            || self.encode_initial_g_front_vowel()
            || self.encode_nger()
            || self.encode_ger()
            || self.encode_gel()
            || self.encode_non_initial_g_front_vowel()
            || self.encode_ga_to_j()
        {
            return;
        }

        if !self.string_at(-1, &["C", "K", "G", "Q"]) {
            self.metaph_add_exact_approx('G', 'K');
        }
    }

    fn encode_silent_g_at_beginning(&mut self) -> bool {
        self.string_at_start(0, &["GN"])
    }

    fn encode_gg(&mut self) -> bool {
        if self.char_next_is('G') {
            // italian e.g, 'loggia', 'caraveggio', also 'suggest' and 'exaggerate'
            if self.string_at(-1, &["AGGIA", "OGGIA", "AGGIO", "EGGIO", "EGGIA", "IGGIO"]) ||
                // 'ruggiero' but not 'snuggies'
                (self.string_at(-1, &["UGGIE"]) && !(self.idx + 3 == self.last_idx || self.idx + 4 == self.last_idx)) ||
                self.string_at_end(-1, &["AGGI", "OGGI"]) ||
                self.string_at(-2, &["SUGGES", "XAGGER", "REGGIE"])
            {
                // exception where "-GG-" => KJ
                if self.string_at(-2, &["SUGGEST"]) {
                    self.metaph_add_exact_approx('G', 'K');
                }
                self.metaph_add('J');
                self.advance_counter(2, 1);
            } else {
                self.metaph_add_exact_approx('G', 'K');
                self.idx += 1;
            }

            return true;
        }

        false
    }

    fn encode_gk(&mut self) -> bool {
        // 'gingko'
        if self.char_next_is('K') {
            self.metaph_add('K');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_gh(&mut self) -> bool {
        if self.char_next_is('H') {
            if self.encode_gh_after_consonant()
                || self.encode_initial_gh()
                || self.encode_gh_to_j()
                || self.encode_gh_to_h()
                || self.encode_ught()
                || self.encode_gh_h_part_of_other_word()
                || self.encode_silent_gh()
                || self.encode_gh_to_f()
            {
                return true;
            }

            self.metaph_add_exact_approx('G', 'K');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_gh_after_consonant(&mut self) -> bool {
        // e.g. 'burgher', 'bingham'
        if self.idx > 0 && !self.is_vowel_at(-1) &&
            // not e.g. 'greenhalgh'
            !self.string_at_end(-3, &["HALGH"])
        {
            self.metaph_add_exact_approx('G', 'K');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_initial_gh(&mut self) -> bool {
        if self.idx == 0 {
            // e.g. "ghislane", "ghiradelli"
            if self.char_at(2, 'I') {
                self.metaph_add('J');
            } else {
                self.metaph_add_exact_approx('G', 'K');
            }
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_gh_to_j(&mut self) -> bool {
        // e.g., 'greenhalgh', 'dunkenhalgh', english names
        if self.string_at_end(-2, &["ALGH"]) {
            self.metaph_add_alt('J', '\0');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_gh_to_h(&mut self) -> bool {
        // special cases
        // e.g., 'donoghue', 'donaghy'
        if (self.string_at(-4, &["DONO", "DONA"]) && self.is_vowel_at(2))
            || self.string_at(-5, &["CALLAGHAN"])
        {
            self.metaph_add('H');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_ught(&mut self) -> bool {
        // e.g. "ought", "aught", "daughter", "slaughter"
        if self.string_at(-1, &["UGHT"]) {
            if (self.string_at(-3, &["LAUGH"])
                && !(self.string_at(-4, &["SLAUGHT"]) || self.string_at(-3, &["LAUGHTO"])))
                || self.string_at(-4, &["DRAUGH"])
            {
                self.metaph_add_str("FT", "FT");
            } else {
                self.metaph_add('T');
            }

            self.idx += 2;
            return true;
        }
        false
    }

    fn encode_gh_h_part_of_other_word(&mut self) -> bool {
        // if the 'H' is the beginning of another word or syllable
        if self.string_at(1, &["HOUS", "HEAD", "HOLE", "HORN", "HARN"]) {
            self.metaph_add_exact_approx('G', 'K');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_silent_gh(&mut self) -> bool {
        // Parker's rule (with some further refinements) - e.g., 'hugh'
        if ((self.string_at(-2, &["B", "H", "D", "G", "L"]) ||
            // e.g., 'bough'
            (self.string_at(-3, &["B", "H", "D", "K", "W", "N", "P", "V"]) && !self.string_start(&["ENOUGH"])) ||
            // e.g., 'broughton'
            // 'plough', 'slaugh'
            self.string_at(-4, &["B", "H", "PL", "SL"]) ||
            (self.idx > 0 && (self.char_at(-1, 'I') || self.string_start(&["PUGH"]) ||
                // e.g. 'MCDONAGH', 'MURTAGH', 'CREAGH'
                self.string_at_end(-1, &["AGH"]) || self.string_at(-4, &["GERAGH", "DRAUGH"]) ||
                (self.string_at(-3, &["GAUGH", "GEOGH", "MAUGH"]) && !self.string_start(&["MCGAUGHEY"])) ||
                // exceptions to 'tough', 'rough', 'lough'
                (self.string_at(-2, &["OUGH"]) && self.idx > 3 &&
                 !self.string_at(-4, &["CCOUGH", "ENOUGH", "TROUGH", "CLOUGH"]))))) &&
            // suffixes starting w/ vowel where "-GH-" is usually silent
            (self.string_at(-3, &["VAUGH", "FEIGH", "LEIGH"]) ||
                self.string_at(-2, &["HIGH", "TIGH"]) ||
                self.idx + 1 == self.last_idx ||
                (self.string_at_end(2, &["IE", "EY", "ES", "ER", "ED", "TY"]) &&
                 !self.string_at(-5, &["GALLAGHER"])) ||
                self.string_at_end(2, &["Y", "ING", "OUT", "ERTY"]) ||
                (!self.is_vowel_at(2) || self.string_at(-3, &["GAUGH", "GEOGH", "MAUGH"]) ||
                 self.string_at(-4, &["BROUGHAM"])))) &&
            // exceptions where '-g-' pronounced
            !(self.string_start(&["BALOGH", "SABAGH"]) || self.string_at(-2, &["BAGHDAD"]) ||
                self.string_at(-3, &["WHIGH"]) || self.string_at(-5, &["SABBAGH", "AKHLAGH"]))
        {
            // silent - do nothing
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_gh_special_cases(&mut self) -> bool {
        let mut handled = false;

        // special case: 'hiccough' == 'hiccup'
        if self.string_at(-6, &["HICCOUGH"]) {
            self.metaph_add('P');
            handled = true;
        } else if self.string_start(&["LOUGH"]) {
            // special case: 'lough' alt spelling for scots 'loch'
            self.metaph_add('K');
            handled = true;
        } else if self.string_start(&["BALOGH"]) {
            // hungarian
            self.metaph_add_exact_approx_alt("G", "", "K", "");
            handled = true;
        } else if self.string_at(-3, &["LAUGHLIN", "COUGHLAN", "LOUGHLIN"]) {
            // "maclaughlin"
            self.metaph_add_alt('K', 'F');
            handled = true;
        } else if self.string_at(-3, &["GOUGH"]) || self.string_at(-7, &["COLCLOUGH"]) {
            self.metaph_add_alt('\0', 'F');
            handled = true;
        }

        if handled {
            self.idx += 1;
        }

        handled
    }

    fn encode_gh_to_f(&mut self) -> bool {
        // the cases covered here would fall under
        // the GH_To_F rule below otherwise
        if self.encode_gh_special_cases() {
            return true;
        }

        // e.g., 'laugh', 'cough', 'rough', 'tough'
        if self.idx > 2
            && self.char_at(-1, 'U')
            && self.is_vowel_at(-2)
            && self.string_at(-3, &["C", "G", "L", "R", "T", "N", "S"])
            && !self.string_at(-4, &["BREUGHEL", "FLAUGHER"])
        {
            self.metaph_add('F');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_silent_g(&mut self) -> bool {
        // e.g. "phlegm", "apothegm", "voigt"
        if self.string_at_end(-1, &["EGM", "IGM", "AGM"])
            || self.string_at_end(0, &["GT"])
            || self.string_exact(&["HUGES"])
        {
            return true;
        }

        // vietnamese names e.g. "Nguyen" but not "Ng"
        if self.string_start(&["NG"]) && self.idx != self.last_idx {
            return true;
        }
        false
    }

    fn encode_gn(&mut self) -> bool {
        if self.char_next_is('N') {
            // 'align' 'sign', 'resign' but not 'resignation'
            // also 'impugn', 'impugnable', but not 'repugnant'
            if (self.idx > 1 &&
                ((self.string_at(-1, &["I", "U", "E"]) ||
                    self.string_at(-3, &["CHAGNON", "LORGNETTE"]) ||
                    self.string_at(-2, &["COGNAC", "LAGNIAPPE"]) ||
                    self.string_at(-4, &["BOLOGN"]) ||
                    self.string_at(-5, &["COMPAGNIE"])) &&
                    // Exceptions: following are cases where 'G' is pronounced
                    // in "assign" 'g' is silent, but not in "assignation"
                    !(self.string_at(2, &["ATE", "ITY", "ATOR", "ATION"]) ||
                        (self.string_at(2, &["AN", "AC", "IA", "UM"]) &&
                         !(self.string_at(-3, &["POIGNANT"]) || self.string_at(-2, &["COGNAC"]))) ||
                        self.string_start(&["SPIGNER", "STEGNER"]) ||
                        self.string_exact(&["SIGNE"]) ||
                        self.string_at(-2, &["LIGNI", "LIGNO", "REGNA", "DIGNI", "WEGNE", "TIGNE",
                            "RIGNE", "REGNE", "TIGNO", "SIGNAL", "SIGNIF", "SIGNAT"]) ||
                        self.string_at(-1, &["IGNIT"])) &&
                    !self.string_at(-2, &["SIGNET", "LIGNEO"]))) ||
                // not e.g. 'cagney', 'magna'
                (self.string_at_end(0, &["GNE", "GNA"]) &&
                 !self.string_at(-2, &["SIGNA", "MAGNA", "SIGNE"]))
            {
                self.metaph_add_exact_approx_alt("N", "GN", "N", "KN");
            } else {
                self.metaph_add_exact_approx_str("GN", "KN");
            }
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_gl(&mut self) -> bool {
        // 'tagliaro', 'puglia' BUT add K in alternative
        // since americans sometimes do this
        if self.string_at(1, &["LIA", "LIO", "LIE"]) && self.is_vowel_at(-1) {
            self.metaph_add_exact_approx_alt("L", "GL", "L", "KL");
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_initial_g_front_vowel(&mut self) -> bool {
        if self.idx == 0 && self.front_vowel(1) {
            // special case "gila" as in "gila monster"
            if self.string_exact(&["GILA"]) {
                self.metaph_add('H');
            } else if self.initial_g_soft() {
                self.metaph_add_exact_approx_alt("J", "G", "J", "K");
            } else if self.char_next_is('E') || self.char_next_is('I') {
                self.metaph_add_exact_approx_alt("G", "J", "K", "J");
            } else {
                self.metaph_add_exact_approx('G', 'K');
            }

            self.advance_counter(1, 0);
            return true;
        }

        false
    }

    fn initial_g_soft(&self) -> bool {
        if (self.string_at(1, &["EL", "EM", "EN", "EO", "ER", "ES", "IA", "IN", "IO", "IP", "IU", "YM", "YN",
            "YP", "YR", "EE", "IRA", "IRO"]) &&
            // except for smaller set of cases where => K, e.g. "gerber"
            !self.string_at(1, &["ELD", "ELT", "ERT", "INZ", "ERH", "ITE", "ERD", "ERL", "ERN", "INT",
                "EES", "EEK", "ELB", "EER", "ERSH", "ERST", "INSB", "INGR", "EROW", "ERKE", "EREN",
                "ELLER", "ERDIE", "ERBER", "ESUND", "ESNER", "INGKO", "INKGO",
                "IPPER", "ESELL", "IPSON", "EEZER", "ERSON", "ELMAN",
                "ESTALT", "ESTAPO", "INGHAM", "ERRITY", "ERRISH", "ESSNER", "ENGLER",
                "YNAECOL", "YNECOLO", "ENTHNER", "ERAGHTY",
                "INGERICH", "EOGHEGAN"]))
            || (self.is_vowel_at(1)
                && (self.string_at(1, &["EE ", "EEW"])
                    || (self.string_at(1, &["IGI", "IRA", "IBE", "AOL", "IDE", "IGL"])
                        && !self.string_at(1, &["IDEON"]))
                    || self.string_at(
                        1,
                        &[
                            "ILES", "INGI", "ISEL", "IBBER", "IBBET", "IBLET", "IBRAN", "IGOLO",
                            "IRARD", "IGANT", "IRAFFE", "EEWHIZ", "ILLETTE", "IBRALTA",
                        ],
                    )
                    || (self.string_at(1, &["INGER"]) && !self.string_at(1, &["INGERICH"]))))
        {
            return true;
        }

        false
    }

    fn front_vowel(&self, offset: isize) -> bool {
        self.char_at(offset, 'E') || self.char_at(offset, 'I') || self.char_at(offset, 'Y')
    }

    fn encode_nger(&mut self) -> bool {
        if self.string_at(-1, &["NGER"]) {
            // default 'G' => J such as 'ranger', 'stranger', 'manger', 'messenger',
            // 'orangery', 'granger'
            // 'boulanger', 'challenger', 'danger', 'changer', 'harbinger', 'lounger',
            // 'ginger', 'passenger'
            // except for these the following

            if !(self.root_or_inflections("ANGER")
                || self.root_or_inflections("LINGER")
                || self.root_or_inflections("MALINGER")
                || self.root_or_inflections("FINGER")
                || (self.string_at(-3, &["HUNG", "FING", "BUNG", "WING", "RING", "DING", "ZENG", "ZING",
                    "JUNG", "LONG", "PING", "CONG", "MONG", "BANG", "GANG", "HANG", "LANG", "SANG", "SING",
                    "WANG", "ZANG"]) &&
                    // exceptions to above where 'G' => J
                    !(self.string_at(-6, &["BOULANG", "SLESING", "KISSING", "DERRING", "BARRING", "PHALANGER"]) ||
                        self.string_at(-8, &["SCHLESING"]) ||
                        self.string_at(-5, &["SALING", "BELANG"]) ||
                        self.string_at(-4, &["CHANG"])))
                || self.string_at(-4, &["STING", "YOUNG"])
                || self.string_at(-5, &["STRONG"])
                || self.string_start(&["UNG", "ENG", "ING", "SENGER"])
                || self.string_at(0, &["GERICH"])
                || self.string_at(-2, &["ANGERLY", "ANGERBO", "INGERSO"])
                || self.string_at(-3, &["WENGER", "MUNGER", "SONGER", "KINGER", "LINGERF"])
                || self.string_at(
                    -4,
                    &[
                        "FLINGER", "SLINGER", "STANGER", "STENGER", "KLINGER", "CLINGER",
                    ],
                )
                || self.string_at(-5, &["SPRINGER", "SPRENGER"]))
            {
                self.metaph_add_exact_approx_alt("J", "G", "J", "K");
            } else {
                self.metaph_add_exact_approx_alt("G", "J", "K", "J");
            }

            self.advance_counter(1, 0);
            return true;
        }
        false
    }

    fn encode_ger(&mut self) -> bool {
        if self.idx > 0 && self.string_at(1, &["ER"]) {
            // Exceptions to 'GE' where 'G' => K
            // e.g. "JAGER", "TIGER", "LIGER", "LAGER", "LUGER", "AUGER", "EAGER", "HAGER",
            // "SAGER"
            if ((self.idx == 2 && self.is_vowel_at(-1) && !self.is_vowel_at(-2) &&
                !self.string_at(-2, &["PAGER", "WAGER", "NIGER", "ROGER", "LEGER", "CAGER"]) ||
                self.string_at(-2, &["AUGER", "EAGER", "INGER", "YAGER"])) ||
                self.string_at(-3, &["SEEGER", "JAEGER", "GEIGER", "KRUGER", "SAUGER", "BURGER",
                    "MEAGER", "MARGER", "RIEGER", "YAEGER", "STEGER", "PRAGER", "SWIGER", "YERGER", "TORGER",
                    "FERGER", "HILGER", "ZEIGER", "YARGER", "COWGER", "CREGER", "KROGER", "KREGER", "GRAGER",
                    "STIGER", "BERGER"]) ||
                // 'berger' but not 'bergerac'
                self.string_at_end(-3, &["BERGER"]) ||
                self.string_at(-4, &["KREIGER", "KRUEGER", "METZGER", "KRIEGER", "KROEGER", "STEIGER",
                    "DRAEGER", "BUERGER", "BOERGER", "FIBIGER"]) ||
                // e.g. 'harshbarger', 'winebarger'
                (self.string_at(-3, &["BARGER"]) && self.idx > 4) ||
                // e.g. 'weisgerber'
                (self.string_at(0, &["GERBER"]) && self.idx > 0) ||
                self.string_at(-5, &["SCHWAGER", "LYBARGER", "SPRENGER", "GALLAGER", "WILLIGER"]) ||
                self.string_start(&["HARGER"]) ||
                self.string_exact(&["AGER", "EGER"]) ||
                self.string_at(-1, &["YGERNE"]) ||
                self.string_at(-6, &["SCHWEIGER"]))
                && !(self.string_at(-5, &["BELLIGEREN"])
                    || self.string_start(&["MARGERY"])
                    || self.string_at(-3, &["BERGERAC"]))
            {
                if self.is_slavo_germanic() {
                    self.metaph_add_exact_approx('G', 'K');
                } else {
                    self.metaph_add_exact_approx_alt("G", "J", "K", "J");
                }
            } else {
                self.metaph_add_exact_approx_alt("J", "G", "J", "K");
            }

            self.advance_counter(1, 0);
            return true;
        }
        false
    }

    fn encode_gel(&mut self) -> bool {
        // more likely to be "-GEL-" => JL
        if self.string_at(1, &["EL"]) && self.idx > 0 {
            // except for
            // "BAGEL", "HEGEL", "HUGEL", "KUGEL", "NAGEL", "VOGEL", "FOGEL", "PAGEL"
            if (self.in_buf.len() == 5 && self.is_vowel_at(-1) && !self.is_vowel_at(-2) &&
                !self.string_at(-2, &["NIGEL", "RIGEL"])) ||
                // or the following as combining forms
                self.string_at(-2, &["ENGEL", "HEGEL", "NAGEL", "VOGEL"]) ||
                self.string_at(-3, &["MANGEL", "WEIGEL", "FLUGEL", "RANGEL", "HAUGEN", "RIEGEL", "VOEGEL"]) ||
                self.string_at(-4, &["SPEIGEL", "STEIGEL", "WRANGEL", "SPIEGEL", "DANEGELD"])
            {
                if self.is_slavo_germanic() {
                    self.metaph_add_exact_approx('G', 'K');
                } else {
                    self.metaph_add_exact_approx_alt("G", "J", "K", "J");
                }
            } else {
                self.metaph_add_exact_approx_alt("J", "G", "J", "K");
            }

            self.advance_counter(1, 0);
            return true;
        }

        false
    }

    // Encode "-G-" followed by a vowel when non-initial letter. Default for this is
    // a 'J' sound, so check exceptions where it is pronounced 'G'
    fn encode_non_initial_g_front_vowel(&mut self) -> bool {
        // -gy-, gi-, ge-
        if self.string_at(1, &["E", "I", "Y"]) {
            // '-ge' at end
            // almost always 'j' sound
            if self.string_at_end(0, &["GE"]) {
                // german names with hard g using GE at end
                if self.string_start(&[
                    "INGE", "LAGE", "HAGE", "LANGE", "SYNGE", "BENGE", "RUNGE", "HELGE", "BYRGE",
                    "BIRGE", "BERGE", "HAUGE", "RENEGE", "STONGE", "STANGE", "PRANGE", "KRESGE",
                ]) {
                    if self.is_slavo_germanic() {
                        self.metaph_add_exact_approx('G', 'K');
                    } else {
                        self.metaph_add_exact_approx_alt("G", "J", "K", "J");
                    }
                } else {
                    self.metaph_add('J');
                }
            } else {
                if self.internal_hard_g() {
                    // don't encode KG or KK if e.g. "mcgill"
                    // todo: should this be !MAC as well?
                    if !self.string_at_start(-2, &["MC"]) || self.string_at_start(-3, &["MAC"]) {
                        if self.is_slavo_germanic() {
                            self.metaph_add_exact_approx('G', 'K');
                        } else {
                            self.metaph_add_exact_approx_alt("G", "J", "K", "J");
                        }
                    }
                } else {
                    self.metaph_add_exact_approx_alt("J", "G", "J", "K");
                }
            }

            self.advance_counter(1, 0);
            return true;
        }

        false
    }

    fn internal_hard_g(&self) -> bool {
        // if not "-GE" at end
        if !(self.idx + 1 == self.last_idx && self.char_next_is('E'))
            && (self.internal_hard_ng()
                || self.internal_hard_gen_gin_get_git()
                || self.internal_hard_g_open_syllable()
                || self.internal_hard_g_other())
        {
            return true;
        }

        false
    }

    fn internal_hard_ng(&self) -> bool {
        if (self.string_at(-3, &["DANG", "FANG", "SING"]) && !self.string_at(-5, &["DISINGEN"]))
            || self.string_start(&["INGEB", "ENGEB"])
            || (self.string_at(-3, &["RING", "WING", "HANG", "LONG"])
                && !(self.string_at(-4, &["CRING", "FRING", "ORANG", "TWING", "CHANG", "PHANG"]) ||
                    self.string_at(-5, &["SYRING"]) ||
                    self.string_at(-3, &["RINGENC", "RINGENT", "LONGITU", "LONGEVI"]) ||
                    // e.g. 'longino', 'mastrangelo'
                    self.string_at_end(0, &["GELO", "GINO"])))
            || (self.string_at(-1, &["NGY"])
                && !(self.string_at(-3, &["RANGY", "MANGY", "MINGY"])
                    || self.string_at(-4, &["SPONGY", "STINGY"])))
        {
            return true;
        }

        false
    }

    fn internal_hard_gen_gin_get_git(&self) -> bool {
        if (self.string_at(
            -3,
            &[
                "FORGET", "TARGET", "MARGIT", "MARGET", "TURGEN", "BERGEN", "MORGEN", "JORGEN",
                "HAUGEN", "JERGEN", "JURGEN", "LINGEN", "BORGEN", "LANGEN", "KLAGEN", "STIGER",
                "BERGER",
            ],
        ) && !self.string_at(0, &["GENETIC", "GENESIS"])
            && !self.string_at(-4, &["PLANGENT"]))
            || self.string_at_end(-3, &["BERGIN", "FEAGIN", "DURGIN"])
            || (self.string_at(-2, &["ENGEN"]) && !self.string_at(3, &["DER", "ETI", "ESI"]))
            || self.string_at(-4, &["JUERGEN"])
            || self.string_start(&["NAGIN", "MAGIN", "HAGIN"])
            || self.string_exact(&["ENGIN", "DEGEN", "LAGEN", "MAGEN", "NAGIN"])
            || (self.string_at(
                -2,
                &[
                    "BEGET", "BEGIN", "HAGEN", "FAGIN", "BOGEN", "WIGIN", "NTGEN", "EIGEN",
                    "WEGEN", "WAGEN",
                ],
            ) && !self.string_at(-5, &["OSPHAGEN"]))
        {
            return true;
        }
        false
    }

    fn internal_hard_g_open_syllable(&self) -> bool {
        self.string_at(1, &["EYE"])
            || self.string_at(-2, &["FOGY", "POGY", "YOGI", "MAGEE", "MCGEE", "HAGIO"])
            || self.string_at(-1, &["RGEY", "OGEY"])
            || self.string_at(-3, &["HOAGY", "STOGY", "PORGY"])
            || self.string_at(-5, &["CARNEGIE"])
            || self.string_at_end(-1, &["OGEY", "OGIE"])
    }

    fn internal_hard_g_other(&self) -> bool {
        if (self.string_at(0, &["GETH", "GEAR", "GEIS", "GIRL", "GIVI", "GIVE", "GIFT", "GIRD", "GIRT", "GILV",
            "GILD", "GELD"]) && !self.string_at(-3, &["GINGIV"])) ||
            // "gish" but not "largish"
            (self.string_at(1, &["ISH"]) && self.idx > 0 && !self.string_start(&["LARG"])) ||
            (self.string_at(-2, &["MAGED", "MEGID"]) && self.idx + 2 != self.last_idx) ||
            self.string_at(0, &["GEZ"]) ||
            self.string_start(&["WEGE", "HAGE", "VOEGE", "BERGE", "HELGE", "INGEBORG", "CORREGIDOR"]) ||
            (self.string_at_end(-2, &["ONGEST", "UNGEST"]) && !self.string_at(-3, &["CONGEST"])) ||
            self.string_exact(&["ENGE", "BOGY"]) ||
            self.string_at(0, &["GIBBON"]) ||
            (self.string_at(0, &["GILL"]) && (self.idx + 3 == self.last_idx || self.idx + 4 == self.last_idx) &&
             !self.string_start(&["STURGILL"]))
        {
            return true;
        }

        false
    }

    fn encode_ga_to_j(&mut self) -> bool {
        // 'margary', 'margarine'
        // but not in spanish forms such as "margarita"
        if (self.string_at(-3, &["MARGARY", "MARGARI"]) && !self.string_at(-3, &["MARGARIT"]))
            || self.string_start(&["GAOL"])
            || self.string_at(-2, &["ALGAE"])
        {
            self.metaph_add_exact_approx_alt("J", "G", "J", "K");
            self.advance_counter(1, 0);
            return true;
        }
        false
    }

    fn encode_h(&mut self) {
        if self.encode_initial_silent_h()
            || self.encode_initial_hs()
            || self.encode_initial_hu_hw()
            || self.encode_non_initial_silent_h()
        {
            return;
        }

        // only keep if first & before vowel or btw. 2 vowels
        self.encode_h_pronounced();
    }

    fn encode_initial_silent_h(&mut self) -> bool {
        // 'hour', 'herb', 'heir', 'honor'
        if self.string_at(1, &["OUR", "ERB", "EIR", "ONOR", "ONOUR", "ONEST"]) {
            // british pronounce H in this word
            // americans give it 'H' for the name, no 'H' for the plant
            if self.string_at_start(0, &["HERB"]) {
                if self.encode_vowels {
                    self.metaph_add_str("HA", "A");
                } else {
                    self.metaph_add_alt('H', 'A');
                }
            } else if self.idx == 0 || self.encode_vowels {
                self.metaph_add('A');
            }

            // don't encode vowels twice
            self.idx = self.skip_vowels(self.idx + 1);
            return true;
        }

        false
    }

    fn encode_initial_hs(&mut self) -> bool {
        // old chinese pinyin transliteration e.g., 'HSIAO'
        if self.string_at_start(0, &["HS"]) {
            self.metaph_add('X');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_initial_hu_hw(&mut self) -> bool {
        // spanish spellings and chinese pinyin transliteration
        if self.string_start(&["HUA", "HUE", "HWA"]) && !self.string_at(0, &["HUEY"]) {
            self.metaph_add('A');

            if !self.encode_vowels {
                self.idx += 2;
            } else {
                self.idx += 1;
                // don't encode vowels twice
                while self.idx < self.length
                    && (Self::is_vowel_char(self.in_buf[self.idx]) || self.in_buf[self.idx] == 'W')
                {
                    self.idx += 1;
                }
                self.idx -= 1; // give back one that's going to be added in the main loop
            }
            return true;
        }

        false
    }

    fn encode_non_initial_silent_h(&mut self) -> bool {
        if self.string_at(
            -2,
            &[
                "NIHIL", "VEHEM", "LOHEN", "NEHEM", "MAHON", "MAHAN", "COHEN", "GAHAN",
            ],
        ) || self.string_at(
            -3,
            &["TOUHY", "GRAHAM", "PROHIB", "FRAHER", "TOOHEY", "TOUHEY"],
        ) || self.string_start(&["CHIHUAHUA"])
        {
            if self.encode_vowels {
                self.idx += 1;
            } else {
                self.idx = self.skip_vowels(self.idx + 1);
            }
            return true;
        }
        false
    }

    fn encode_h_pronounced(&mut self) -> bool {
        if ((self.idx == 0 || self.is_vowel_at(-1) || (self.idx > 0 && self.char_at(-1, 'W'))) &&
            self.is_vowel_at(1)) ||
            // e.g. 'alWahhab'
            (self.char_next_is('H') && self.is_vowel_at(2))
        {
            self.metaph_add('H');
            self.advance_counter(1, 0);
            return true;
        }

        false
    }

    fn encode_j(&mut self) {
        if self.encode_spanish_j() || self.encode_spanish_oj_uj() {
            return;
        }

        if self.idx == 0 {
            if !self.encode_german_j() {
                self.encode_j_to_j();
            }
        } else {
            if self.encode_spanish_j2() {
                return;
            } else if !self.encode_j_as_vowel() {
                self.metaph_add('J');
            }

            // eat redundant 'J'
            if self.char_next_is('J') {
                self.idx += 1;
            }
        }
    }

    fn encode_spanish_j(&mut self) -> bool {
        // Obvious spanish, e.g. "jose", "san jacinto"
        if (self.string_at(1, &["UAN", "ACI", "ALI", "EFE", "ICA", "IME", "OAQ", "UAR"])
            && !self.string_at(0, &["JIMERSON", "JIMERSEN"]))
            || self.string_at_end(1, &["OSE"])
            || self.string_at(
                1,
                &["EREZ", "UNTA", "AIME", "AVIE", "AVIA", "IMINEZ", "ARAMIL"],
            )
            || self.string_at_end(-2, &["MEJIA"])
            || self.string_at(
                -2,
                &[
                    "TEJED", "TEJAD", "LUJAN", "FAJAR", "BEJAR", "BOJOR", "CAJIG", "DEJAS",
                    "DUJAR", "DUJAN", "MIJAR", "MEJOR", "NAJAR", "NOJOS", "RAJED", "RIJAL",
                    "REJON", "TEJAN", "UIJAN",
                ],
            )
            || self.string_at(-3, &["ALEJANDR", "GUAJARDO", "TRUJILLO"])
            || (self.string_at(-2, &["RAJAS"]) && self.idx > 2)
            || (self.string_at(-2, &["MEJIA"]) && !self.string_at(-2, &["MEJIAN"]))
            || self.string_at(-1, &["OJEDA"])
            || self.string_at(-3, &["LEIJA", "MINJA", "VIAJES", "GRAJAL"])
            || self.string_at(0, &["JAUREGUI"])
            || self.string_at(-4, &["HINOJOSA"])
            || self.string_start(&["SAN "])
            || ((self.idx + 1 == self.last_idx)
                && self.char_at(1, 'O')
                && !self.string_start(&["TOJO", "BANJO", "MARYJO"]))
        {
            if !(self.string_at(0, &["JUAN"]) || self.string_at(0, &["JOAQ"])) {
                self.metaph_add('H');
            } else if self.idx == 0 {
                self.metaph_add('A');
            }
            self.advance_counter(1, 0);
            return true;
        }

        // Jorge gets 2nd HARHA, also JULIO, JESUS
        if self.string_at(1, &["ORGE", "ULIO", "ESUS"]) && !self.string_start(&["JORGEN"]) {
            if self.string_at_end(1, &["ORGE"]) {
                if self.encode_vowels {
                    self.metaph_add_str("JARJ", "HARHA");
                } else {
                    self.metaph_add_str("JRJ", "HRH");
                }
                self.advance_counter(4, 4);
                return true;
            }
            self.metaph_add_alt('J', 'H');
            self.advance_counter(1, 0);
            return true;
        }

        false
    }

    fn encode_german_j(&mut self) -> bool {
        if self.string_at(1, &["AH", "UGO"])
            || self.string_exact(&["JOHANN"])
            || (self.string_at(1, &["UNG"]) && !self.char_at(4, 'L'))
        {
            self.metaph_add('A');
            self.advance_counter(1, 0);
            return true;
        }
        false
    }

    fn encode_spanish_oj_uj(&mut self) -> bool {
        if self.string_at(1, &["OJOBA", "UJUY"]) {
            if self.encode_vowels {
                self.metaph_add_str("HAH", "HAH");
            } else {
                self.metaph_add_str("HH", "HH");
            }
            self.advance_counter(3, 2);
            return true;
        }
        false
    }

    fn encode_j_to_j(&mut self) -> bool {
        if self.is_vowel_at(1) {
            if self.idx == 0 && self.names_beginning_with_j_that_get_alt_y() {
                // 'Y' is a vowel so encode is as 'A'
                if self.encode_vowels {
                    self.metaph_add_str("JA", "A");
                } else {
                    self.metaph_add_alt('J', 'A');
                }
            } else {
                if self.encode_vowels {
                    self.metaph_add_str("JA", "JA");
                } else {
                    self.metaph_add('J');
                }
            }
            self.idx = self.skip_vowels(self.idx + 1);
            return false;
        }

        self.metaph_add('J');
        true
    }

    fn encode_spanish_j2(&mut self) -> bool {
        // Spanish forms e.g. "brujo", "badajoz"
        if self.string_at_start(
            -2,
            &["BOJA", "BAJA", "BEJA", "BOJO", "MOJA", "MOJI", "MEJI"],
        ) || self.string_at_start(
            -3,
            &[
                "FRIJO", "BRUJO", "BRUJA", "GRAJE", "GRIJA", "LEIJA", "QUIJA",
            ],
        ) || self.string_at_end(
            -1,
            &[
                "AJOS", "EJOS", "OJAS", "OJOS", "UJON", "AJOZ", "AJAL", "UJAR", "EJON", "EJAN",
                "AJARA",
            ],
        ) || (self.string_at_end(-1, &["OJA", "EJA"]) && !self.string_start(&["DEJA"]))
        {
            self.metaph_add('H');
            self.advance_counter(1, 0);
            return true;
        }
        false
    }

    fn encode_j_as_vowel(&mut self) -> bool {
        if self.string_at(0, &["JEWSK"]) {
            self.metaph_add_alt('J', '\0'); // J in primary only, nothing in secondary
            return true;
        }

        // e.g. "stijl", "sejm"
        if (self.string_at(1, &["L", "T", "K", "S", "N", "M"]) && !self.string_at(2, &["A"]))
            || self.string_start(&[
                "FJ",
                "WOJ",
                "LJUB",
                "BJOR",
                "HAJEK",
                "HALLELUJA",
                "LJUBLJANA",
            ])
            || self.string_at(0, &["JAVIK", "JEVIC"])
            || self.string_exact(&["SONJA", "TANJA", "TONJA"])
        {
            return true;
        }
        false
    }

    fn names_beginning_with_j_that_get_alt_y(&self) -> bool {
        // Full list from Go - checking if name starts with J and matches common names
        self.string_start(&[
            "JAN",
            "JON",
            "JIN",
            "JEN",
            "JUHL",
            "JULY",
            "JOEL",
            "JOHN",
            "JOSH",
            "JUDE",
            "JUNE",
            "JONI",
            "JULI",
            "JENA",
            "JUNG",
            "JINA",
            "JANA",
            "JENI",
            "JANN",
            "JONA",
            "JENE",
            "JULE",
            "JANI",
            "JONG",
            "JEAN",
            "JONE",
            "JARA",
            "JUST",
            "JOST",
            "JAHN",
            "JACO",
            "JANG",
            "JOANN",
            "JANEY",
            "JANAE",
            "JOANA",
            "JUTTA",
            "JULEE",
            "JANAY",
            "JANEE",
            "JETTA",
            "JOHNA",
            "JOANE",
            "JAYNA",
            "JANES",
            "JONAS",
            "JONIE",
            "JUSTA",
            "JUNIE",
            "JUNKO",
            "JENAE",
            "JULIO",
            "JINNY",
            "JOHNS",
            "JACOB",
            "JETER",
            "JAFFE",
            "JESKE",
            "JANKE",
            "JAGER",
            "JANIK",
            "JANDA",
            "JOSHI",
            "JULES",
            "JANTZ",
            "JEANS",
            "JUDAH",
            "JANUS",
            "JENNY",
            "JENEE",
            "JONAH",
            "JOSUE",
            "JOSEF",
            "JULIE",
            "JULIA",
            "JANIE",
            "JANIS",
            "JENNA",
            "JANNA",
            "JEANA",
            "JENNI",
            "JEANE",
            "JONNA",
            "JAKOB",
            "JORDAN",
            "JORDON",
            "JOSEPH",
            "JOSHUA",
            "JOSIAH",
            "JOSPEH",
            "JUDSON",
            "JULIAN",
            "JULIUS",
            "JUNIOR",
            "JUDITH",
            "JOESPH",
            "JOHNIE",
            "JOANNE",
            "JEANNE",
            "JOANNA",
            "JOSEFA",
            "JULIET",
            "JANNIE",
            "JANELL",
            "JASMIN",
            "JANINE",
            "JOHNNY",
            "JEANIE",
            "JEANNA",
            "JOHNNA",
            "JOELLE",
            "JOVITA",
            "JONNIE",
            "JANEEN",
            "JANINA",
            "JOANIE",
            "JAZMIN",
            "JANENE",
            "JONELL",
            "JENELL",
            "JANETT",
            "JANETH",
            "JENINE",
            "JOELLA",
            "JOEANN",
            "JOHANA",
            "JENICE",
            "JANNET",
            "JANISE",
            "JULENE",
            "JANEAN",
            "JAIMEE",
            "JOETTE",
            "JANYCE",
            "JENEVA",
            "JACOBS",
            "JENSEN",
            "JANSEN",
            "JAEGER",
            "JACOBY",
            "JENSON",
            "JARMAN",
            "JOSLIN",
            "JESSEN",
            "JAHNKE",
            "JACOBO",
            "JULIEN",
            "JEPSON",
            "JANSON",
            "JACOBI",
            "JARBOE",
            "JOHSON",
            "JANZEN",
            "JETTON",
            "JUNKER",
            "JONSON",
            "JAROSZ",
            "JENNER",
            "JAGGER",
            "JEPSEN",
            "JORDEN",
            "JANNEY",
            "JUHASZ",
            "JERGEN",
            "JOHNSON",
            "JOHNNIE",
            "JASMINE",
            "JEANNIE",
            "JOHANNA",
            "JANELLE",
            "JANETTE",
            "JULIANA",
            "JUSTINA",
            "JOSETTE",
            "JOELLEN",
            "JENELLE",
            "JULIETA",
            "JULIANN",
            "JULISSA",
            "JENETTE",
            "JANETTA",
            "JOSELYN",
            "JONELLE",
            "JESENIA",
            "JANESSA",
            "JAZMINE",
            "JEANENE",
            "JOANNIE",
            "JADWIGA",
            "JOLANDA",
            "JULIANE",
            "JANUARY",
            "JEANICE",
            "JANELLA",
            "JEANETT",
            "JENNINE",
            "JOHANNE",
            "JOHNSIE",
            "JANIECE",
            "JENNELL",
            "JAMISON",
            "JANSSEN",
            "JOHNSEN",
            "JARDINE",
            "JAGGERS",
            "JURGENS",
            "JOURDAN",
            "JULIANO",
            "JOSEPHS",
            "JHONSON",
            "JOZWIAK",
            "JANICKI",
            "JELINEK",
            "JANSSON",
            "JOACHIM",
            "JACOBUS",
            "JENNING",
            "JANTZEN",
            "JOSEFINA",
            "JEANNINE",
            "JULIANNE",
            "JULIANNA",
            "JONATHAN",
            "JONATHON",
            "JEANETTE",
            "JANNETTE",
            "JEANETTA",
            "JOHNETTA",
            "JENNEFER",
            "JULIENNE",
            "JOSPHINE",
            "JEANELLE",
            "JOHNETTE",
            "JULIEANN",
            "JOSEFINE",
            "JULIETTA",
            "JOHNSTON",
            "JACOBSON",
            "JACOBSEN",
            "JOHANSEN",
            "JOHANSON",
            "JAWORSKI",
            "JENNETTE",
            "JELLISON",
            "JOHANNES",
            "JASINSKI",
            "JUERGENS",
            "JARNAGIN",
            "JEREMIAH",
            "JEPPESEN",
            "JARNIGAN",
            "JANOUSEK",
            "JOHNATHAN",
            "JOHNATHON",
            "JORGENSEN",
            "JEANMARIE",
            "JOSEPHINA",
            "JEANNETTE",
            "JOSEPHINE",
            "JEANNETTA",
            "JORGENSON",
            "JANKOWSKI",
            "JOHNSTONE",
            "JABLONSKI",
            "JOSEPHSON",
            "JOHANNSEN",
            "JURGENSEN",
            "JIMMERSON",
            "JOHANSSON",
            "JAKUBOWSKI",
        ])
    }

    fn encode_k(&mut self) {
        if !self.encode_silent_k() {
            self.metaph_add('K');

            // eat redundant K's and Q's
            if self.char_at(1, 'K') || self.char_at(1, 'Q') {
                self.idx += 1;
            }
        }
        // Don't increment idx - let main loop handle it
    }

    fn encode_silent_k(&mut self) -> bool {
        if self.idx == 0
            && self.string_start(&["KN"])
            && !self.string_at(2, &["ISH", "ESSET", "IEVEL"])
        {
            return true;
        }

        // e.g. "know", "knit", "knob"
        if (self.string_at(1, &["NOW", "NIT", "NOT", "NOB"]) && !self.string_start(&["BANKNOTE"]))
            || self.string_at(1, &["NOCK", "NUCK", "NIFE", "NACK", "NIGHT"])
        {
            // N already encoded before
            // e.g. "penknife"
            if self.idx > 0 && self.char_at(-1, 'N') {
                self.idx += 1;
            }

            return true;
        }

        false
    }

    fn encode_l(&mut self) {
        // logic below needs to know this
        // after 'idx' variable changed
        let save_idx = self.idx;

        self.interpolate_vowel_when_cons_l_at_end();

        if self.encode_lely_to_l()
            || self.encode_colonel()
            || self.encode_french_ault()
            || self.encode_french_euil()
            || self.encode_french_oulx()
            || self.encode_silent_l_in_lm()
            || self.encode_silent_l_in_lk_lv()
            || self.encode_silent_l_in_ould()
        {
            return;
        }

        if self.encode_ll_as_vowel_cases() {
            return;
        }

        self.encode_le_cases(save_idx);
    }

    // Cases where an L follows D, G, or T at the end have a schwa pronounced before
    // the L
    fn interpolate_vowel_when_cons_l_at_end(&mut self) {
        // e.g. "ertl", "vogl"
        if self.encode_vowels && self.string_at_end(-1, &["DL", "GL", "TL"]) {
            self.metaph_add('A');
        }
    }

    fn encode_lely_to_l(&mut self) -> bool {
        // e.g. "agilely", "docilely"
        if self.string_at_end(-1, &["ILELY"]) {
            self.metaph_add('L');
            self.idx += 2;
            return true;
        }
        false
    }

    fn encode_colonel(&mut self) -> bool {
        if self.string_at(-2, &["COLONEL"]) {
            self.metaph_add('R');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_french_ault(&mut self) -> bool {
        // e.g. "renault" and "foucault", well known to americans, but not "fault"
        if self.idx > 3
            && (self.string_at(-3, &["RAULT", "NAULT", "BAULT", "SAULT", "GAULT", "CAULT"])
                || self.string_at(-4, &["REAULT", "RIAULT", "NEAULT", "BEAULT"]))
            && !(self.root_or_inflections("ASSAULT")
                || self.string_at(-8, &["SOMERSAULT"])
                || self.string_at(-9, &["SUMMERSAULT"]))
        {
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_french_euil(&mut self) -> bool {
        // e.g. "auteuil"
        if self.string_at_end(-3, &["EUIL"]) {
            return true;
        }
        false
    }

    fn encode_french_oulx(&mut self) -> bool {
        // e.g. "proulx"
        if self.string_at_end(-2, &["OULX"]) {
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_silent_l_in_lm(&mut self) -> bool {
        if self.string_at(0, &["LM", "LN"]) {
            // e.g. "lincoln", "holmes", "psalm", "salmon"
            if (self.string_at(-2, &["COLN", "CALM", "BALM", "MALM", "PALM"])
                || self.string_at_end(-1, &["OLM"])
                || self.string_at(-3, &["PSALM", "QUALM"])
                || self.string_at(-2, &["SALMON", "HOLMES"])
                || self.string_at(-1, &["ALMOND"])
                || self.string_at_start(-1, &["ALMS"]))
                && (!self.string_at(2, &["A"])
                    && !self.string_at(-2, &["BALMO", "PALMER", "PALMOR", "BALMER"])
                    && !self.string_at(-3, &["THALM"]))
            {
                // silent - do nothing
            } else {
                self.metaph_add('L');
            }

            return true;
        }

        false
    }

    fn encode_silent_l_in_lk_lv(&mut self) -> bool {
        if (self.string_at(
            -2,
            &[
                "WALK", "YOLK", "FOLK", "HALF", "TALK", "CALF", "BALK", "CALK",
            ],
        ) || (self.string_at(-2, &["POLK", "HALV", "SALVE", "CALVE", "SOLDER"])
            && !self.string_at(
                -2,
                &["POLKA", "PALKO", "HALVA", "HALVO", "SALVER", "CALVER"],
            ))
            || (self.string_at(-3, &["CAULK", "CHALK", "BAULK", "FAULK"])
                && !self.string_at(-4, &["SCHALK"])))
            && !self.string_at(-5, &["GONSALVES", "GONCALVES"])
            && !self.string_at(-2, &["BALKAN", "TALKAL"])
            && !self.string_at(-3, &["PAULK", "CHALF"])
        {
            return true;
        }

        false
    }

    fn encode_silent_l_in_ould(&mut self) -> bool {
        // 'would', 'could'
        if self.string_at(-3, &["WOULD", "COULD"])
            || (self.string_at(-4, &["SHOULD"]) && !self.string_at(-4, &["SHOULDER"]))
        {
            self.metaph_add_exact_approx('D', 'T');
            self.idx += 1;
            return true;
        }
        false
    }

    // Encode "-ILLA-" and "-ILLE-" in spanish and french contexts where americans
    // know to pronounce it as a 'Y'
    fn encode_ll_as_vowel_special_cases(&mut self) -> bool {
        if self.string_at(-5, &["TORTILLA"]) || self.string_at(-8, &["RATATOUILLE"]) ||
            // e.g. 'guillermo', "veillard"
            (self.string_start(&["GUILL", "VEILL", "GAILL"]) &&
                // 'guillotine' usually has '-ll-' pronounced as 'L' in english
                !(self.string_at(-3, &["GUILLOT", "GUILLOR", "GUILLEN"]) ||
                  self.string_exact(&["GUILL"]))) ||
            // e.g. "brouillard", "gremillion"
            self.string_start(&["ROBILL", "BROUILL", "GREMILL"]) ||
            // e.g. 'mireille'
            // exception "reveille" usually pronounced as 're-vil-lee'
            (self.string_at_end(-2, &["EILLE"]) && !self.string_at(-5, &["REVEILLE"]))
        {
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_ll_as_vowel(&mut self) -> bool {
        // spanish e.g. "cabrillo", "gallegos" but also "gorilla", "ballerina" -
        // give both pronunciations since an american might pronounce "cabrillo"
        // in the spanish or the american fashion.
        if self.string_at_end(-1, &["ILLO", "ILLA", "ALLE"])
            || (self.string_end(&["A", "O", "AS", "OS"])
                && self.string_at(-1, &["AL", "IL"])
                && !self.string_at(-1, &["ALLA"]))
            || self.string_start(&[
                "LLA", "VILLE", "VILLA", "GALLARDO", "VALLADAR", "MAGALLAN", "CAVALLAR", "BALLASTE",
            ])
        {
            self.metaph_add_alt('L', '\0');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_ll_as_vowel_cases(&mut self) -> bool {
        if self.char_next_is('L') {
            if self.encode_ll_as_vowel_special_cases() || self.encode_ll_as_vowel() {
                return true;
            }
            self.idx += 1;
        }

        false
    }

    fn encode_vowel_le_transposition(&mut self, idx: usize) -> bool {
        // transposition of vowel sound and L occurs in many words,
        // e.g. "bristle", "dazzle", "goggle" => KAKAL
        let offset = (self.idx as isize) - (idx as isize);
        if self.encode_vowels && idx > 1 && !self.is_vowel_at(offset - 1) &&
            self.char_at(offset + 1, 'E') &&
            !self.char_at(offset - 1, 'L') && !self.char_at(offset - 1, 'R') &&
            // lots of exceptions to this:
            !self.is_vowel_at(offset + 2) &&
            !self.string_start(&["MCCLE", "MCLEL", "EMBLEM", "KADLEC", "ECCLESI", "COMPLEC", "COMPLEJ", "ROBLEDO"]) &&
            !(idx + 2 == self.last_idx && self.string_at(offset, &["LET"])) &&
            !self.string_at(offset, &["LEG", "LER", "LEX", "LESS", "LESQ", "LECT", "LEDG", "LETE", "LETH", "LETS", "LETT",
                "LETUS", "LETIV", "LETELY", "LETTER", "LETION", "LETIAN", "LETING", "LETORY", "LETTING"]) &&
            // e.g. "complement" !=> KAMPALMENT
            !(self.string_at(offset, &["LEMENT"]) &&
                !(self.string_at(-4, &["BATTLE", "TANGLE", "PUZZLE", "RABBLE", "BABBLE"]) ||
                  self.string_at(-3, &["TABLE"]))) &&
            !(idx + 2 == self.last_idx && self.string_at(offset - 2, &["OCLES", "ACLES", "AKLES"])) &&
            !self.string_at(offset - 3, &["LISLE", "AISLE"]) && !self.string_start(&["ISLE"]) &&
            !self.string_start(&["ROBLES"]) &&
            !self.string_at(offset - 4, &["PROBLEM", "RESPLEN"]) &&
            !self.string_at(offset - 3, &["REPLEN"]) &&
            !self.string_at(offset - 2, &["SPLE"]) &&
            !self.char_at(offset - 1, 'H') && !self.char_at(offset - 1, 'W')
        {
            self.metaph_add_str("AL", "AL");
            self.flag_al_inversion = true;

            // eat redundant 'L'
            if self.char_at(offset + 2, 'L') {
                self.idx = idx + 2;
            }
            return true;
        }

        false
    }

    fn encode_vowel_preserve_vowel_after_l(&mut self, idx: usize) -> bool {
        let offset = (idx as isize) - (self.idx as isize);

        if self.encode_vowels
            && !self.is_vowel_at(offset - 1)
            && self.char_at(offset + 1, 'E')
            && idx > 1
            && idx + 1 != self.last_idx
            && !(self.string_at(offset + 1, &["ES", "ED"]) && idx + 2 == self.last_idx)
            && !self.string_at(offset - 1, &["RLEST"])
        {
            self.metaph_add_str("LA", "LA");
            self.idx = self.skip_vowels(self.idx + 1);
            return true;
        }

        false
    }

    fn encode_le_cases(&mut self, idx: usize) {
        if self.encode_vowel_le_transposition(idx) {
            return;
        }

        if self.encode_vowel_preserve_vowel_after_l(idx) {
            return;
        }
        self.metaph_add('L');
    }

    fn encode_m(&mut self) {
        if self.encode_silent_m_at_beginning()
            || self.encode_mr_and_mrs()
            || self.encode_mac()
            || self.encode_mpt()
        {
            return;
        }

        // Silent 'B' should really be handled
        // under 'B", not here under 'M'!
        self.encode_mb();

        self.metaph_add('M');
    }

    fn encode_silent_m_at_beginning(&mut self) -> bool {
        self.string_at_start(0, &["MN"])
    }

    fn encode_mr_and_mrs(&mut self) -> bool {
        if self.string_exact(&["MR"]) {
            if self.encode_vowels {
                self.metaph_add_str("MASTAR", "MASTAR");
            } else {
                self.metaph_add_str("MSTR", "MSTR");
            }
            self.idx += 1;
            return true;
        } else if self.string_exact(&["MRS"]) {
            if self.encode_vowels {
                self.metaph_add_str("MASAS", "MASAS");
            } else {
                self.metaph_add_str("MSS", "MSS");
            }
            self.idx += 2;
            return true;
        }

        false
    }

    fn encode_mac(&mut self) -> bool {
        // should only find irish and
        // scottish names e.g. 'macintosh'
        if self.string_at_start(
            0,
            &[
                "MC",
                "MACIVER",
                "MACEWEN",
                "MACELROY",
                "MACILROY",
                "MACINTOSH",
            ],
        ) {
            if self.encode_vowels {
                self.metaph_add_str("MAK", "MAK");
            } else {
                self.metaph_add_str("MK", "MK");
            }

            if self.string_start(&["MC"]) {
                // watch out for e.g. "McGeorge"
                if self.string_at(2, &["K", "G", "Q"]) && !self.string_at(2, &["GEOR"]) {
                    self.idx += 2;
                } else {
                    self.idx += 1;
                }
            } else {
                self.idx += 2;
            }

            return true;
        }

        false
    }

    fn encode_mpt(&mut self) -> bool {
        if self.string_at(-2, &["COMPTROL"]) || self.string_at(-4, &["ACCOMPT"]) {
            self.metaph_add('N');
            self.idx += 1;
            return true;
        }

        false
    }

    fn test_silent_mb_1(&self) -> bool {
        // e.g. "LAMB", "COMB", "LIMB", "DUMB", "BOMB"
        // Handle combining roots first
        self.string_at_start(-3, &["THUMB"])
            || self.string_at_start(-2, &["DUMB", "BOMB", "DAMN", "LAMB", "NUMB", "TOMB"])
    }

    fn test_pronounced_mb(&self) -> bool {
        self.string_at(-2, &["NUMBER"])
            || (self.string_at(2, &["A", "O"]) && !self.string_at(-2, &["DUMBASS"]))
            || self.string_at(-2, &["LAMBEN", "LAMBER", "LAMBET", "TOMBIG", "LAMBRE"])
    }

    fn test_silent_mb_2(&self) -> bool {
        // 'M' is the current letter
        self.char_next_is('B')
            && self.idx > 1
            && (self.idx + 1 == self.last_idx
                || // other situations where "-MB-" is at end of root
                // but not at end of word. The tests are for standard
                // noun suffixes.
                // e.g. "climbing" => KLMNK
                self.string_at(2, &["ING", "ABL", "LIKE"])
                || self.string_at_end(2, &["S"])
                || self.string_at(-5, &["BUNCOMB"])
                || //e.g. "bomber"
                (self.string_at_end(2, &["ED", "ER"])
                    && (self.string_start(&["CLIMB", "PLUMB"])
                        || !self.string_at(-1, &["IMBER", "AMBER", "EMBER", "UMBER"]))
                    && !self.string_at(-2, &["CUMBER", "SOMBER"])))
    }

    fn test_pronounced_mb_2(&self) -> bool {
        // e.g. "bombastic", "umbrage", "flamboyant"
        self.string_at(-1, &["OMBAS", "OMBAD", "UMBRA"]) || self.string_at(-3, &["FLAM"])
    }

    fn test_mn(&self) -> bool {
        self.char_next_is('N')
            && (self.idx + 1 == self.last_idx
                || // or at the end of a word but followed by suffixes
                self.string_at_end(2, &["S", "LY", "ER", "ED", "ING", "EST"])
                || self.string_at(-2, &["DAMNEDEST"])
                || self.string_at(-5, &["GODDAMNIT"]))
    }

    fn encode_mb(&mut self) {
        if self.test_silent_mb_1() {
            if !self.test_pronounced_mb() {
                self.idx += 1;
            }
        } else if self.test_silent_mb_2() {
            if !self.test_pronounced_mb_2() {
                self.idx += 1;
            }
        } else if self.test_mn() || self.char_next_is('M') {
            self.idx += 1;
        }
    }

    fn encode_n(&mut self) {
        if self.encode_nce() {
            return;
        }

        //eat redundant 'N'
        if self.char_next_is('N') {
            self.idx += 1;
        }

        // e.g. "aloneness",
        if !self.string_at(-2, &["MONSIEUR"]) && !self.string_at(-2, &["NENESS"]) {
            self.metaph_add('N');
        }
    }

    //Encode "-NCE-" and "-NSE-" "entrance" is pronounced exactly the same as
    //"entrants"
    fn encode_nce(&mut self) -> bool {
        // 'acceptance', 'accountancy'
        if self.string_at(1, &["C", "S"])
            && self.string_at(2, &["E", "Y", "I"])
            && (self.idx + 2 == self.last_idx
                || (self.idx + 3 == self.last_idx && self.char_at(3, 'S')))
        {
            self.metaph_add_str("NTS", "NTS");
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_p(&mut self) {
        if self.encode_silent_p_at_beginning()
            || self.encode_pt()
            || self.encode_ph()
            || self.encode_pph()
            || self.encode_rps()
            || self.encode_coup()
            || self.encode_pneum()
            || self.encode_psych()
            || self.encode_psalm()
        {
            return;
        }

        self.encode_pb();

        self.metaph_add('P');
    }

    fn encode_silent_p_at_beginning(&mut self) -> bool {
        self.string_at_start(0, &["PN", "PF", "PS", "PT"])
    }

    fn encode_pt(&mut self) -> bool {
        // 'pterodactyl', 'receipt', 'asymptote'
        if self.char_next_is('T')
            && (self.string_at_start(0, &["PTERO"])
                || self.string_at(-5, &["RECEIPT"])
                || self.string_at(-4, &["ASYMPTOT"]))
        {
            self.metaph_add('T');
            self.idx += 1;
            return true;
        }

        false
    }

    //Encode "-PH-", usually as F, with exceptions for cases where it is silent, or
    //where the 'P' and 'T' are pronounced seperately because they belong to two
    //different words in a combining form
    fn encode_ph(&mut self) -> bool {
        if self.char_next_is('H') {
            // 'PH' silent in these contexts
            if self.string_at(0, &["PHTHALEIN"])
                || self.string_at_start(0, &["PHTH"])
                || self.string_at(-3, &["APOPHTHEGM"])
            {
                self.metaph_add('0');
                self.idx += 3;
            } else if self.idx > 0
                && (self.string_at(
                    2,
                    &[
                        "AM", "EAD", "OLE", "ELD", "ILL", "OLD", "EAP", "ERD", "ARD", "ANG", "ORN",
                        "EAV", "ART", "OUSE", "AMMER", "AZARD", "UGGER", "OLSTER",
                    ],
                ) && !self.string_at(-1, &["LPHAM"]))
                && !self.string_at(-3, &["LYMPH", "NYMPH"])
            {
                // combining forms
                // 'sheepherd', 'upheaval', 'cupholder'
                self.metaph_add('P');
                self.advance_counter(2, 1);
            } else {
                self.metaph_add('F');
                self.idx += 1;
            }

            return true;
        }

        false
    }

    fn encode_pph(&mut self) -> bool {
        // 'sappho'
        if self.char_next_is('P') && self.idx + 2 < self.length && self.char_at(2, 'H') {
            self.metaph_add('F');
            self.idx += 2;
            return true;
        }
        false
    }

    fn encode_rps(&mut self) -> bool {
        // '-corps-', 'corpsman'
        if self.string_at(-3, &["CORPS"]) && !self.string_at(-3, &["CORPSE"]) {
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_coup(&mut self) -> bool {
        // 'coup'
        self.string_at_end(-3, &["COUP"]) && !self.string_at(-5, &["RECOUP"])
    }

    fn encode_pneum(&mut self) -> bool {
        // '-pneum-'
        if self.string_at(1, &["NEUM"]) {
            self.metaph_add('N');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_psych(&mut self) -> bool {
        // '-psych-'
        if self.string_at(1, &["SYCH"]) {
            if self.encode_vowels {
                self.metaph_add_str("SAK", "SAK");
            } else {
                self.metaph_add_str("SK", "SK");
            }
            self.idx += 4;
            return true;
        }
        false
    }

    fn encode_psalm(&mut self) -> bool {
        if self.string_at(1, &["SALM"]) {
            if self.encode_vowels {
                self.metaph_add_str("SAM", "SAM");
            } else {
                self.metaph_add_str("SM", "SM");
            }
            self.idx += 4;
            return true;
        }
        false
    }

    fn encode_pb(&mut self) {
        // e.g. "campbell", "raspberry"
        // eat redundant 'P' or 'B'
        if self.string_at(1, &["P", "B"]) {
            self.idx += 1;
        }
    }

    fn encode_q(&mut self) {
        // current pinyin
        if self.string_at(0, &["QIN"]) {
            self.metaph_add('X');
            return;
        }

        // eat redundant 'Q'
        if self.char_next_is('Q') {
            self.idx += 1;
        }
        self.metaph_add('K');
    }

    fn encode_r(&mut self) {
        if self.encode_rz() {
            return;
        }

        if !self.test_silent_r() && !self.encode_vowel_re_transposition() {
            self.metaph_add('R');
        }

        // eat redundant 'R'; also skip 'S' as well as 'R' in "poitiers"
        if self.char_next_is('R') || self.string_at(-6, &["POITIERS"]) {
            self.idx += 1;
        }
    }

    //Encode "-RZ-" according to american and polish pronunciations
    fn encode_rz(&mut self) -> bool {
        if self.string_at(
            -2,
            &["GARZ", "KURZ", "MARZ", "MERZ", "HERZ", "PERZ", "WARZ"],
        ) || self.string_at(0, &["RZANO", "RZOLA"])
            || self.string_at(-1, &["ARZA", "ARZN"])
        {
            return false;
        }

        // 'yastrzemski' usually has 'z' silent in
        // united states, but should get 'X' in poland
        if self.string_at(-4, &["YASTRZEMSKI"]) {
            self.metaph_add_alt('R', 'X');
            self.idx += 1;
            return true;
        }

        // 'BRZEZINSKI' gets two pronunciations
        // in the united states, neither of which
        // are authentically polish
        if self.string_at(-1, &["BRZEZINSKI"]) {
            self.metaph_add_str("RS", "RJ");
            //skip of 2nd Z
            self.idx += 3;
            return true;
        }

        // 'z' in 'rz after voiceless consonant gets 'X'
        // in alternate polish style pronunciation
        if self.string_at(-1, &["TRZ", "PRZ", "KRZ"])
            || (self.string_at(0, &["RZ"]) && (self.is_vowel_at(-1) || self.idx == 0))
        {
            self.metaph_add_str("RS", "X");
            self.idx += 1;
            return true;
        }

        // 'z' in 'rz after voiceled consonant, vowel, or at
        // beginning gets 'J' in alternate polish style pronunciation
        if self.string_at(-1, &["BRZ", "DRZ", "GRZ"]) {
            self.metaph_add_str("RS", "J");
            self.idx += 1;
            return true;
        }

        false
    }

    fn test_silent_r(&self) -> bool {
        // test cases where 'R' is silent, either because the
        // word is from the french or because it is no longer pronounced.
        // e.g. "rogier", "monsieur", "surburban"
        if (self.idx == self.last_idx
            && self.string_at(-2, &["IER"])
            && // e.g. "metier"
            (self.string_at(-5, &["MET", "VIV", "LUC"])
                || // e.g. "cartier", "bustier"
                self.string_at(
                    -6,
                    &[
                        "CART", "DOSS", "FOUR", "OLIV", "BUST", "DAUM", "ATEL", "SONN", "CORM",
                        "MERC", "PELT", "POIR", "BERN", "FORT", "GREN", "SAUC", "GAGN", "GAUT",
                        "GRAN", "FORC", "MESS", "LUSS", "MEUN", "POTH", "HOLL", "CHEN",
                    ],
                )
                || // e.g. "croupier"
                self.string_at(
                    -7,
                    &["CROUP", "TORCH", "CLOUT", "FOURN", "GAUTH", "TROTT", "DEROS", "CHART"],
                )
                || // e.g. "chevalier"
                self.string_at(
                    -8,
                    &["CHEVAL", "LAVOIS", "PELLET", "SOMMEL", "TREPAN", "LETELL", "COLOMB"],
                )
                || self.string_at(-9, &["CHARCUT"])
                || self.string_at(-10, &["CHARPENT"])))
            || self.string_at(-2, &["SURBURB", "WORSTED", "WORCESTER"])
            || self.string_at(-7, &["MONSIEUR"])
            || self.string_at(-6, &["POITIERS"])
        {
            return true;
        }

        false
    }

    //Encode '-re-" as 'AR' in contexts where this is the correct pronunciation
    fn encode_vowel_re_transposition(&mut self) -> bool {
        // -re inversion is just like
        // -le inversion
        // e.g. "fibre" => FABAR or "centre" => SANTAR
        if self.encode_vowels
            && self.char_next_is('E')
            && self.length > 3
            && !self.string_start(&["OUTRE", "LIBRE", "ANDRE"])
            && !self.string_exact(&["FRED", "TRES"])
            && !self.string_at(-2, &["LDRED", "LFRED", "NDRED", "NFRED", "NDRES", "IFRED"]) //"TRES" ?
            && !self.is_vowel_at(-1)
            && (self.idx + 1 == self.last_idx || self.string_at_end(2, &["D", "S"]))
        {
            self.metaph_add_str("AR", "AR");
            return true;
        }

        false
    }

    fn encode_s(&mut self) {
        if self.encode_skj()
            || self.encode_special_sw()
            || self.encode_sj()
            || self.encode_silent_french_s_final()
            || self.encode_silent_french_s_internal()
            || self.encode_isl()
            || self.encode_stl()
            || self.encode_christmas()
            || self.encode_sthm()
            || self.encode_isten()
            || self.encode_sugar()
            || self.encode_sh()
            || self.encode_sch()
            || self.encode_sur()
            || self.encode_su()
            || self.encode_ssio()
            || self.encode_ss()
            || self.encode_sia()
            || self.encode_sio()
            || self.encode_anglicisations()
            || self.encode_sc()
            || self.encode_sei_sui_sier()
            || self.encode_sea()
        {
            return;
        }

        self.metaph_add('S');

        if self.string_at(1, &["S", "Z"]) && !self.string_at(1, &["SH"]) {
            self.idx += 1;
        }
    }

    fn encode_skj(&mut self) -> bool {
        if self.string_at(0, &["SKJO", "SKJU"]) && self.is_vowel_at(3) {
            self.metaph_add('X');
            self.idx += 2;
            return true;
        }
        false
    }

    fn encode_special_sw(&mut self) -> bool {
        if self.idx == 0 {
            if self.names_beginning_with_sw_that_get_alt_sv() {
                self.metaph_add_str("S", "SV");
                self.idx += 1;
                return true;
            }

            if self.names_beginning_with_sw_that_get_alv_xv() {
                self.metaph_add_str("S", "XV");
                self.idx += 1;
                return true;
            }
        }
        false
    }

    fn names_beginning_with_sw_that_get_alt_sv(&self) -> bool {
        self.string_start(&[
            "SWANSON",
            "SWENSON",
            "SWINSON",
            "SWENSEN",
            "SWOBODA",
            "SWIDERSKI",
            "SWARTHOUT",
            "SWEARENGIN",
        ])
    }

    fn names_beginning_with_sw_that_get_alv_xv(&self) -> bool {
        self.string_start(&[
            "SWART",
            "SWARTZ",
            "SWARTS",
            "SWIGER",
            "SWITZER",
            "SWANGER",
            "SWIGERT",
            "SWIGART",
            "SWIHART",
            "SWEITZER",
            "SWATZELL",
            "SWINDLER",
            "SWINEHART",
            "SWEARINGEN",
        ])
    }

    fn encode_sj(&mut self) -> bool {
        if self.string_start(&["SJ"]) {
            self.metaph_add('X');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_silent_french_s_final(&mut self) -> bool {
        // "louis" is an exception because it gets two pronunciations
        if self.string_start(&["LOUIS"]) && self.idx == self.last_idx {
            self.metaph_add_alt('S', '\0');
            return true;
        }

        if self.idx == self.last_idx
            && ((self.string_start(&[
                "YVES",
                "ARKANSAS",
                "FRANCAIS",
                "CRUDITES",
                "BRUYERES",
                "DESCARTES",
                "DESCHUTES",
                "DESCHAMPS",
                "DESROCHES",
                "DESCHENES",
                "RENDEZVOUS",
                "CONTRETEMPS",
                "DESLAURIERS",
            ]) || self.string_exact(&["HORS"])
                || self.string_end(&[
                    "CAMUS", "YPRES", "MESNES", "DEBRIS", "BLANCS", "INGRES", "CANNES", "CHABLIS",
                    "APROPOS", "JACQUES", "ELYSEES", "OEUVRES", "GEORGES", "DESPRES",
                ]))
                || (self.string_at(-2, &["AI", "OI", "UI"])
                    && !self.string_start(&["LOIS", "LUIS"])))
        {
            return true;
        }
        false
    }

    fn encode_silent_french_s_internal(&mut self) -> bool {
        // french words familiar to americans where internal s is silent
        self.string_at(
            -2,
            &[
                "MESNES",
                "DESCHAM",
                "DESPRES",
                "DESROCH",
                "DESROSI",
                "DESJARD",
                "DESMARA",
                "DESCHEN",
                "DESHOTE",
                "DESLAUR",
                "DESCARTES",
            ],
        ) || self.string_at(-5, &["DUQUESNE", "DUCHESNE"])
            || self.string_at(-3, &["FRESNEL", "GROSVENOR"])
            || self.string_at(-4, &["LOUISVILLE"])
            || self.string_at(-7, &["BEAUCHESNE", "ILLINOISAN"])
    }

    fn encode_isl(&mut self) -> bool {
        // special cases 'island', 'isle', 'carlisle', 'carlysle'
        (self.string_at(-2, &["LISL", "LYSL", "AISL"])
            && !self.string_at(-3, &["PAISLEY", "BAISLEY", "ALISLAM", "ALISLAH", "ALISLAA"]))
            || (self.idx == 1
                && (self.string_at(-1, &["ISLE", "ISLAN"])
                    && !self.string_at(-1, &["ISLEY", "ISLER"])))
    }

    fn encode_stl(&mut self) -> bool {
        // 'hustle', 'bustle', 'whistle'
        if (self.string_at(0, &["STLE", "STLI"]) && !self.string_at(2, &["LESS", "LIKE", "LINE"])) ||
            self.string_at(-3, &["THISTLY", "BRISTLY", "GRISTLY"]) ||
            // e.g. "corpuscle"
            self.string_at(-1, &["USCLE"])
        {
            // KRISTEN, KRYSTLE, CRYSTLE, KRISTLE all pronounce the 't'
            // also, exceptions where "-LING" is a nominalizing suffix
            if self.string_start(&[
                "KRISTEN",
                "KRYSTLE",
                "CRYSTLE",
                "KRISTLE",
                "CHRISTENSEN",
                "CHRISTENSON",
            ]) || self.string_at(-3, &["FIRSTLING"])
                || self.string_at(-2, &["NESTLING", "WESTLING"])
            {
                self.metaph_add_str("ST", "ST");
                self.idx += 1;
            } else {
                if self.encode_vowels
                    && self.char_at(3, 'E')
                    && !self.char_at(4, 'R')
                    && !self.string_at(3, &["EY", "ETTE", "ETTA"])
                {
                    self.metaph_add_str("SAL", "SAL");
                    self.flag_al_inversion = true;
                } else {
                    self.metaph_add_str("SL", "SL");
                }
                self.idx += 2;
            }
            return true;
        }

        false
    }

    fn encode_christmas(&mut self) -> bool {
        if self.string_at(-4, &["CHRISTMA"]) {
            self.metaph_add_str("SM", "SM");
            self.idx += 2;
            return true;
        }
        false
    }

    fn encode_sthm(&mut self) -> bool {
        // 'asthma', 'isthmus'
        if self.string_at(0, &["STHM"]) {
            self.metaph_add_str("SM", "SM");
            self.idx += 3;
            return true;
        }
        false
    }

    fn encode_isten(&mut self) -> bool {
        // 't' is silent in verb, pronounced in name
        if self.string_start(&["CHRISTEN"]) {
            if self.root_or_inflections("CHRISTEN") || self.string_start(&["CHRISTENDOM"]) {
                self.metaph_add_str("S", "ST");
            } else {
                // e.g. 'christenson', 'christene'
                self.metaph_add_str("ST", "ST");
            }
            self.idx += 1;
            return true;
        }

        // e.g. 'glisten', 'listen'
        if self.string_at(-2, &["LISTEN", "RISTEN", "HASTEN", "FASTEN", "MUSTNT"])
            || self.string_at(-3, &["MOISTEN"])
        {
            self.metaph_add('S');
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_sugar(&mut self) -> bool {
        if self.string_at(0, &["SUGAR"]) {
            self.metaph_add('X');
            return true;
        }
        false
    }

    fn encode_sh(&mut self) -> bool {
        if self.string_at(0, &["SH"]) {
            // exception
            if self.string_at(-2, &["CASHMERE"]) {
                self.metaph_add('J');
                self.idx += 1;
                return true;
            }

            // combining forms, e.g. 'clotheshorse', 'woodshole'
            if self.idx > 0
                && (self.string_at_end(1, &["HAP"]) ||
                    // e.g. "hartsheim", "clothshorse"
                    // e.g. "dishonor"
                    self.string_at(1, &["HEIM", "HOEK", "HOLM", "HOLZ", "HOOD", "HEAD", "HEID",
                        "HAAR", "HORS", "HOLE", "HUND", "HELM", "HAWK", "HILL", "HEART", "HATCH", "HOUSE", "HOUND", "HONOR"]) ||
                    // e.g. "mishear"
                    self.string_at_end(2, &["EAR"]) ||
                    // e.g. "hartshorn"
                    (self.string_at(2, &["ORN"]) && !self.string_at(-2, &["UNSHORN"])) ||
                    // e.g. "newshour" but not "bashour", "manshour"
                    (self.string_at(1, &["HOUR"]) && !self.string_start(&["ASHOUR", "BASHOUR", "MANSHOUR"])) ||
                    // e.g. "dishonest", "grasshopper"
                    self.string_at(2, &["ARMON", "ONEST", "ALLOW", "OLDER", "OPPER", "EIMER",
                        "ANDLE", "ONOUR", "ABILLE", "UMANCE", "ABITUA"]))
            {
                if !self.string_at(-1, &["S"]) {
                    self.metaph_add('S');
                }
            } else {
                self.metaph_add('X');
            }

            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_sch(&mut self) -> bool {
        // these words were combining forms many centuries ago
        if self.string_at(1, &["CH"]) {
            if self.idx > 0 &&
                // e.g. "mischief", "escheat"
                (self.string_at(3, &["IEF", "EAT", "ANCE", "ARGE"]) ||
                    self.string_start(&["ESCHEW"]))
            {
                self.metaph_add('S');
                return true;
            }

            // Schlesinger's rule
            // dutch, danish, italian, greek origin, e.g. "school", "schooner", "schiavone",
            // "schiz-"
            if (self.string_at(
                3,
                &["OO", "ER", "EN", "UY", "ED", "EM", "IA", "IZ", "IS", "OL"],
            ) && !self.string_at(0, &["SCHOLT", "SCHISL", "SCHERR"]))
                || self.string_at(3, &["ISZ"])
                || (self.string_at(-1, &["ESCHAT", "ASCHIN", "ASCHAL", "ISCHAE", "ISCHIA"])
                    && !self.string_at(-2, &["FASCHING"]))
                || self.string_at_end(-1, &["ESCHI"])
                || self.char_at(3, 'Y')
            {
                // e.g. "schermerhorn", "schenker", "schistose"

                if self.string_at(3, &["ER", "EN", "IS"])
                    && (self.idx + 4 == self.last_idx || self.string_at(3, &["ENK", "ENB", "IST"]))
                {
                    self.metaph_add_str("X", "SK");
                } else {
                    self.metaph_add_str("SK", "SK");
                }
            } else {
                self.metaph_add('X');
            }

            self.idx += 2;
            return true;
        }

        false
    }

    fn encode_sur(&mut self) -> bool {
        // 'erasure', 'usury'
        if self.string_at(1, &["URE", "URA", "URY"]) {
            // 'sure', 'ensure'
            if self.idx == 0 || self.string_at(-1, &["N", "K"]) || self.string_at(-2, &["NO"]) {
                self.metaph_add('X');
            } else {
                self.metaph_add('J');
            }

            self.advance_counter(1, 0);
            return true;
        }
        false
    }

    fn encode_su(&mut self) -> bool {
        // 'sensuous', 'consensual'
        if self.string_at(1, &["UO", "UA"]) && self.idx != 0 {
            // exceptions e.g. "persuade"
            if self.string_at(-1, &["RSUA"]) {
                self.metaph_add('S');
            } else if self.is_vowel_at(-1) {
                // exceptions e.g. "casual"
                self.metaph_add_alt('J', 'S');
            } else {
                self.metaph_add_alt('X', 'S');
            }

            self.advance_counter(2, 0);
            return true;
        }
        false
    }

    fn encode_ssio(&mut self) -> bool {
        if self.string_at(1, &["SION"]) {
            // "abcission"
            if self.string_at(-2, &["CI"]) {
                self.metaph_add('J');
            } else if self.is_vowel_at(-1) {
                // 'mission'
                self.metaph_add('X');
            }

            self.advance_counter(3, 1);
            return true;
        }
        false
    }

    fn encode_ss(&mut self) -> bool {
        // e.g. "russian", "pressure"
        // e.g. "hessian", "assurance"
        if self.string_at(
            -1,
            &[
                "USSIA", "ESSUR", "ISSUR", "ISSUE", "ESSIAN", "ASSURE", "ASSURA", "ISSUAB",
                "ISSUAN", "ASSIUS",
            ],
        ) {
            self.metaph_add('X');
            self.advance_counter(2, 1);
            return true;
        }
        false
    }

    fn encode_sia(&mut self) -> bool {
        // e.g. "controversial", also "fuchsia", "ch" is silent
        if self.string_at(-2, &["CHSIA"]) || self.string_at(-1, &["RSIAL"]) {
            self.metaph_add('X');
            self.advance_counter(2, 0);
            return true;
        }

        // names generally get 'X' where terms, e.g. "aphasia" get 'J'
        if (self.string_at_start(-3, &["ALESIA", "ALYSIA", "ALISIA", "STASIA"])
            && !self.string_start(&["ANASTASIA"]))
            || self.string_at(-5, &["THERESIA", "DIONYSIAN"])
        {
            self.metaph_add_alt('X', 'S');
            self.advance_counter(2, 0);
            return true;
        }

        if self.string_at_end(0, &["SIA", "SIAN"]) || self.string_at(-5, &["AMBROSIAL"]) {
            if (self.is_vowel_at(-1) || self.string_at(-1, &["R"])) &&
                // exclude compounds based on names, or french or greek words
                !(self.string_start(&["JAMES", "NICOS", "PEGAS", "PEPYS",
                    "HOBBES", "HOLMES", "JAQUES", "KEYNES",
                    "MALTHUS", "HOMOOUS", "MAGLEMOS", "HOMOIOUS",
                    "LEVALLOIS", "TARDENOIS"]) || self.string_at(-4, &["ALGES"]))
            {
                self.metaph_add('J');
            } else {
                self.metaph_add('S');
            }

            self.advance_counter(1, 0);
            return true;
        }

        false
    }

    fn encode_sio(&mut self) -> bool {
        // special case, irish name
        if self.string_start(&["SIOBHAN"]) {
            self.metaph_add('X');
            self.advance_counter(2, 0);
            return true;
        }
        if self.string_at(1, &["ION"]) {
            // e.g. "vision", "version"
            if self.is_vowel_at(-1) || self.string_at(-2, &["ER", "UR"]) {
                self.metaph_add('J');
            } else {
                // e.g. "declension"
                self.metaph_add('X');
            }
            self.advance_counter(2, 0);
            return true;
        }
        false
    }

    fn encode_anglicisations(&mut self) -> bool {
        // german & anglicisations, e.g. 'smith' match 'schmidt', 'snider' match
        // 'schneider'
        // also, -sz- in slavic language altho in hungarian it is pronounced 's'

        if self.string_at_start(0, &["SM", "SN", "SL"]) || self.string_at(1, &["Z"]) {
            self.metaph_add_alt('S', 'X');

            // eat redundant 'Z'
            if self.string_at(1, &["Z"]) {
                self.idx += 1;
            }

            return true;
        }

        false
    }

    fn encode_sc(&mut self) -> bool {
        if self.string_at(0, &["SC"]) {
            // exception 'viscount'
            if self.string_at(-2, &["VISCOUNT"]) {
                return true;
            }

            // encode "-SC<front vowel>-"
            if self.string_at(2, &["I", "E", "Y"]) {
                // e.g. "conscious"
                // e.g. "prosciutto"
                if self.string_at(2, &["IUT", "IOUS"])
                    || self.string_at(-2, &["FASCIS"])
                    || self.string_at(-3, &["CONSCIEN", "CRESCEND", "CONSCION"])
                    || self.string_at(-4, &["OMNISCIEN"])
                {
                    self.metaph_add('X');
                } else if self.string_at(0, &["SCIVV", "SCIRO", "SCIPIO", "SCEPTIC", "SCEPSIS"])
                    || self.string_at(-2, &["PISCITELLI"])
                {
                    self.metaph_add_str("SK", "SK");
                } else {
                    self.metaph_add('S');
                }

                self.idx += 1;
                return true;
            }

            self.metaph_add_str("SK", "SK");
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_sei_sui_sier(&mut self) -> bool {
        // "nausea" by itself has => NJ as a more likely encoding. Other forms
        // using "nause-" (see encode_sea()) have X or S as more familiar
        // pronunciations
        if self.string_at_end(-3, &["NAUSEA"])
            || self.string_at(-2, &["CASUI"])
            || (self.string_at(-1, &["OSIER", "ASIER"])
                && !(self.string_start(&["OSIER", "EASIER"])
                    || self.string_at(-2, &["ROSIER", "MOSIER"])))
        {
            self.metaph_add_alt('J', 'X');
            self.advance_counter(2, 0);
            return true;
        }

        false
    }

    fn encode_sea(&mut self) -> bool {
        //TODO: bug?  NAUSEO and not NAUSEAT?
        if self.string_exact(&["SEAN"])
            || (self.string_at(-3, &["NAUSEO"]) && !self.string_at(-3, &["NAUSEAT"]))
        {
            self.metaph_add('X');
            self.advance_counter(2, 0);
            return true;
        }
        false
    }

    fn encode_t(&mut self) {
        if self.encode_t_initial()
            || self.encode_tch()
            || self.encode_silent_french_t()
            || self.encode_tun_tul_tua_tuo()
            || self.encode_tue_teu_teou_tul_tie()
            || self.encode_tur_tiu_suffixes()
            || self.encode_ti()
            || self.encode_tient()
            || self.encode_tsch()
            || self.encode_tzsch()
            || self.encode_th_pronounced_separately()
            || self.encode_tth()
            || self.encode_th()
        {
            return;
        }

        if self.string_at(1, &["T", "D"]) {
            self.idx += 1;
        }
        self.metaph_add('T');
    }

    fn encode_t_initial(&mut self) -> bool {
        if self.idx == 0 {
            // americans usually pronounce "tzar" as "zar"
            if self.string_at(1, &["SAR", "ZAR"]) {
                return true;
            }

            // old 'École française d'Extrême-Orient' chinese pinyin where 'ts-' => 'X'
            if self.string_exact(&["TSO", "TSA", "TSU", "TSAO", "TSAI", "TSING", "TSANG"]) {
                self.metaph_add('X');
                self.advance_counter(2, 1);
                return true;
            }

            // "TS<vowel>-" at start can be pronounced both with and without 'T'
            if self.char_next_is('S') && self.is_vowel_at(2) {
                self.metaph_add_str("TS", "S");
                self.advance_counter(2, 1);
                return true;
            }

            // e.g. "Tjaarda"
            if self.char_next_is('J') {
                self.metaph_add('X');
                self.advance_counter(2, 1);
                return true;
            }

            if self.string_exact(&["THU"])
                || self.string_at(1, &["HAI", "HUY", "HAO", "HYME", "HYMY", "HANH", "HERES"])
            {
                self.metaph_add('T');
                self.advance_counter(2, 1);
                return true;
            }
        }

        false
    }

    fn encode_tch(&mut self) -> bool {
        if self.string_at(1, &["CH"]) {
            self.metaph_add('X');
            self.idx += 2;
            return true;
        }
        false
    }

    fn encode_silent_french_t(&mut self) -> bool {
        // french silent T familiar to americans
        (self.string_at_end(-4, &["MONET", "GENET", "CHAUT"])
            || self.string_at(-2, &["POTPOURRI"])
            || self.string_at(-3, &["MORTGAGE", "BOATSWAIN"])
            || self.string_at(
                -4,
                &[
                    "BERET", "BIDET", "FILET", "DEBUT", "DEPOT", "PINOT", "TAROT",
                ],
            )
            || self.string_at(
                -5,
                &[
                    "BALLET", "BUFFET", "CACHET", "CHALET", "ESPRIT", "RAGOUT", "GOULET", "CHABOT",
                    "BENOIT",
                ],
            )
            || self.string_at(
                -6,
                &[
                    "GOURMET", "BOUQUET", "CROCHET", "CROQUET", "PARFAIT", "PINCHOT", "CABARET",
                    "PARQUET", "RAPPORT", "TOUCHET", "COURBET", "DIDEROT",
                ],
            )
            || self.string_at(
                -7,
                &[
                    "ENTREPOT", "CABERNET", "DUBONNET", "MASSENET", "MUSCADET", "RICOCHET",
                    "ESCARGOT",
                ],
            )
            || self.string_at(
                -8,
                &[
                    "SOBRIQUET",
                    "CABRIOLET",
                    "CASSOULET",
                    "OUBRIQUET",
                    "CAMEMBERT",
                ],
            ))
            && !self.string_at(1, &["AN", "RY", "IC", "OM", "IN"])
    }

    fn encode_tun_tul_tua_tuo(&mut self) -> bool {
        // e.g. "fortune", "fortunate"
        if self.string_at(-3, &["FORTUN"])
            || // e.g. "capitulate"
            (self.string_at(0, &["TUL"]) && self.is_vowel_at(-1) && self.is_vowel_at(3))
            || // e.g. "obituary", "barbituate"
            self.string_at(-2, &["BITUA", "BITUE"])
            || // e.g. "actual"
            (self.idx > 1 && self.string_at(0, &["TUA", "TUO"]))
        {
            self.metaph_add_alt('X', 'T');
            return true;
        }
        false
    }

    fn encode_tue_teu_teou_tul_tie(&mut self) -> bool {
        if self.string_at(1, &["UENT"])
            || self.string_at(-4, &["RIGHTEOUS"])
            || self.string_at(-3, &["STATUTE", "AMATEUR", "STATUTOR"])
            || // e.g. "blastula", "pasteur"
            self.string_at(-1, &["NTULE", "NTULA", "STULE", "STULA", "STEUR"])
            || // e.g. "statue"
            self.string_at_end(0, &["TUE"])
            || // e.g. "constituency"
            self.string_at(0, &["TUENC"])
            || // e.g. "patience"
            self.string_at_end(0, &["TIENCE"])
        {
            self.metaph_add_alt('X', 'T');
            self.advance_counter(1, 0);
            return true;
        }

        false
    }

    fn encode_tur_tiu_suffixes(&mut self) -> bool {
        // 'adventure', 'musculature'
        if self.idx > 0 && self.string_at(1, &["URE", "URA", "URI", "URY", "URO", "IUS"]) {
            // exceptions e.g. 'tessitura', mostly from romance languages
            if (self.string_at_end(1, &["URA", "URO"]) && !self.string_at(-3, &["VENTURA"]))
                || // e.g. "kachaturian", "hematuria"
                self.string_at(1, &["URIA"])
            {
                self.metaph_add('T');
            } else {
                self.metaph_add_alt('X', 'T');
            }

            self.advance_counter(1, 0);
            return true;
        }
        false
    }

    fn encode_ti(&mut self) -> bool {
        // '-tio-', '-tia-', '-tiu-'
        // except combining forms where T already pronounced e.g 'rooseveltian'
        if (self.string_at(1, &["IO"]) && !self.string_at(-1, &["ETIOL"]))
            || self.string_at(1, &["IAL"])
            || self.string_at(-1, &["RTIUM", "ATIUM"])
            || ((self.string_at(1, &["IAN"]) && self.idx > 0)
                && !(self.string_at(-4, &["FAUSTIAN"])
                    || self.string_at(-5, &["PROUSTIAN"])
                    || self.string_at(-2, &["TATIANA"])
                    || self.string_at(-3, &["KANTIAN", "GENTIAN"])
                    || self.string_at(-8, &["ROOSEVELTIAN"]))
                || (self.string_at_end(0, &["TIA"])
                    && // exceptions to above rules where the pronounciation is usually X
                    !(self.string_at(-3, &["HESTIA", "MASTIA"])
                        || self.string_at(-2, &["OSTIA"])
                        || self.string_start(&["TIA"])
                        || self.string_at(-5, &["IZVESTIA"])))
                || self.string_at(1, &["IATE", "IATI", "IABL", "IATO", "IARY"])
                || self.string_at(-5, &["CHRISTIAN"]))
        {
            if self.string_at_start(-2, &["ANTI"])
                || self.string_start(&["PATIO", "PITIA", "DUTIA"])
            {
                self.metaph_add('T');
            } else if self.string_at(-4, &["EQUATION"]) {
                self.metaph_add('J');
            } else if self.string_at(0, &["TION"]) {
                self.metaph_add('X');
            } else if self.string_start(&["KATIA", "LATIA"]) {
                self.metaph_add_alt('T', 'X');
            } else {
                self.metaph_add_alt('X', 'T');
            }

            self.advance_counter(2, 0);
            return true;
        }

        false
    }

    fn encode_tient(&mut self) -> bool {
        // e.g. 'patient'
        if self.string_at(1, &["IENT"]) {
            self.metaph_add_alt('X', 'T');
            self.advance_counter(2, 0);
            return true;
        }
        false
    }

    fn encode_tsch(&mut self) -> bool {
        // 'deutsch'
        if self.string_at(0, &["TSCH"])
            && // combining forms in german where the 'T' is pronounced seperately
            !self.string_at(-3, &["WELT", "KLAT", "FEST"])
        {
            // pronounced the same as "ch" in "chit" => X
            self.metaph_add('X');
            self.idx += 3;
            return true;
        }
        false
    }

    fn encode_tzsch(&mut self) -> bool {
        // 'neitzsche'
        if self.string_at(0, &["TZSCH"]) {
            self.metaph_add('X');
            self.idx += 4;
            return true;
        }
        false
    }

    fn encode_th_pronounced_separately(&mut self) -> bool {
        // 'adulthood', 'bithead', 'apartheid'
        if (self.idx > 0
            && self.string_at(
                1,
                &[
                    "HOOD", "HEAD", "HEID", "HAND", "HILL", "HOLD", "HAWK", "HEAP", "HERD", "HOLE",
                    "HOOK", "HUNT", "HUMO", "HAUS", "HOFF", "HARD",
                ],
            )
            && !self.string_at(-3, &["SOUTH", "NORTH"]))
            || self.string_at(1, &["HOUSE", "HEART", "HASTE", "HYPNO", "HEQUE"])
            || // watch out for greek root "-thallic"
            (self.string_at_end(1, &["HALL"]) && !self.string_at(-3, &["SOUTH", "NORTH"]))
            || (self.string_at_end(1, &["HAM"])
                && !self.string_start(&[
                    "GOTHAM", "WITHAM", "LATHAM", "BENTHAM", "WALTHAM", "WORTHAM", "GRANTHAM",
                ]))
            || (self.string_at(1, &["HATCH"]) && !(self.idx == 0 || self.string_at(-2, &["UNTHATCH"])))
            || self.string_at(-3, &["GOETHE", "WARTHOG"])
            || // and some special cases where "-TH-" is usually pronounced 'T'
            self.string_at(-2, &["ESTHER", "NATHALIE"])
        {
            //special case
            if self.string_at(-3, &["POSTHUM"]) {
                self.metaph_add('X');
            } else {
                self.metaph_add('T');
            }
            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_tth(&mut self) -> bool {
        // 'matthew' vs. 'outthink'
        if self.string_at(0, &["TTH"]) {
            if self.string_at(-2, &["MATTH"]) {
                self.metaph_add('0');
            } else {
                self.metaph_add_str("T0", "T0");
            }
            self.idx += 2;
            return true;
        }

        false
    }

    fn encode_th(&mut self) -> bool {
        if self.string_at(0, &["TH"]) {
            // '-clothes-'
            if self.string_at(-3, &["CLOTHES"]) {
                // vowel already encoded so skip right to S
                self.idx += 2;
                return true;
            }

            // special case "thomas", "thames", "beethoven" or germanic words
            if self.string_at(
                2,
                &[
                    "OMAS", "OMPS", "OMPK", "OMSO", "OMSE", "AMES", "OVEN", "OFEN", "ILDA", "ILDE",
                ],
            ) || self.string_exact(&["THOM", "THOMS"])
                || self.string_start(&["SCH", "VAN ", "VON "])
            {
                self.metaph_add('T');
            } else {
                // give an 'etymological' 2nd
                // encoding for "smith"
                if self.string_start(&["SM"]) {
                    self.metaph_add_alt('0', 'T');
                } else {
                    self.metaph_add('0');
                }
            }

            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_v(&mut self) {
        if self.char_next_is('V') {
            self.idx += 1;
        }
        self.metaph_add_exact_approx('V', 'F');
    }

    fn encode_w(&mut self) {
        if self.encode_silent_w_at_beginning()
            || self.encode_witz_wicz()
            || self.encode_wr()
            || self.encode_initial_w_vowel()
            || self.encode_wh()
            || self.encode_eastern_european_w()
        {
            return;
        }

        // e.g. 'zimbabwe'
        if self.encode_vowels && self.string_at_end(0, &["WE"]) {
            self.metaph_add('A');
        }
    }

    fn encode_silent_w_at_beginning(&mut self) -> bool {
        self.string_at_start(0, &["WR"])
    }

    fn encode_witz_wicz(&mut self) -> bool {
        // polish e.g. 'filipowicz'
        if self.string_at_end(0, &["WICZ", "WITZ"]) {
            if self.encode_vowels {
                // don't dupe A's
                if !self.prim_buf.is_empty() && self.prim_buf[self.prim_buf.len() - 1] == 'A' {
                    self.metaph_add_str("TS", "FAX");
                } else {
                    self.metaph_add_str("ATS", "FAX");
                }
            } else {
                self.metaph_add_str("TS", "FX");
            }

            self.idx += 3;
            return true;
        }
        false
    }

    fn encode_wr(&mut self) -> bool {
        // can also be in middle of word
        if self.string_at(0, &["WR"]) {
            self.metaph_add('R');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_initial_w_vowel(&mut self) -> bool {
        if self.idx == 0 && self.is_vowel_at(1) {
            // Witter should match Vitter
            if self.germanic_or_slavic_name_beginning_with_w() {
                if self.encode_vowels {
                    self.metaph_add_exact_approx_alt("A", "VA", "A", "FA");
                } else {
                    self.metaph_add_exact_approx_alt("A", "V", "A", "F");
                }
            } else {
                self.metaph_add('A');
            }

            self.idx = self.skip_vowels(self.idx + 1);
            return true;
        }

        false
    }

    fn encode_wh(&mut self) -> bool {
        if self.string_at(0, &["WH"]) {
            // cases where it is pronounced as H
            // e.g. 'who', 'whole'
            if self.char_at(2, 'O')
                && !self.string_at(2, &["OA", "OP", "OOP", "OMP", "ORL", "ORT", "OOSH"])
            {
                self.metaph_add('H');
                self.advance_counter(2, 1);
                return true;
            }

            // combining forms, e.g. 'hollowhearted', 'rawhide'
            if self.string_at(
                2,
                &[
                    "IDE", "ARD", "EAD", "AWK", "ERD", "OOK", "AND", "OLE", "OOD", "EART", "OUSE",
                    "OUND", "AMMER",
                ],
            ) {
                self.metaph_add('H');
                self.idx += 1;
                return true;
            }

            if self.idx == 0 {
                self.metaph_add('A');
                self.idx = self.skip_vowels(self.idx + 2);
                return true;
            }

            self.idx += 1;
            return true;
        }

        false
    }

    fn encode_eastern_european_w(&mut self) -> bool {
        // Arnow should match Arnoff
        if (self.idx == self.last_idx && self.is_vowel_at(-1))
            || self.string_at(-1, &["EWSKI", "EWSKY", "OWSKI", "OWSKY"])
            || self.string_at_end(0, &["WIAK", "WICKI", "WACKI"])
            || self.string_start(&["SCH"])
        {
            self.metaph_add_exact_approx_alt("", "V", "", "F");
            return true;
        }
        false
    }

    fn germanic_or_slavic_name_beginning_with_w(&self) -> bool {
        self.string_start(&[
            "WEE",
            "WIX",
            "WAX",
            "WOLF",
            "WEIS",
            "WAHL",
            "WALZ",
            "WEIL",
            "WERT",
            "WINE",
            "WILK",
            "WALT",
            "WOLL",
            "WADA",
            "WULF",
            "WEHR",
            "WURM",
            "WYSE",
            "WENZ",
            "WIRT",
            "WOLK",
            "WEIN",
            "WYSS",
            "WASS",
            "WANN",
            "WINT",
            "WINK",
            "WILE",
            "WIKE",
            "WIER",
            "WELK",
            "WISE",
            "WIRTH",
            "WIESE",
            "WITTE",
            "WENTZ",
            "WOLFF",
            "WENDT",
            "WERTZ",
            "WILKE",
            "WALTZ",
            "WEISE",
            "WOOLF",
            "WERTH",
            "WEESE",
            "WURTH",
            "WINES",
            "WARGO",
            "WIMER",
            "WISER",
            "WAGER",
            "WILLE",
            "WILDS",
            "WAGAR",
            "WERTS",
            "WITTY",
            "WIENS",
            "WIEBE",
            "WIRTZ",
            "WYMER",
            "WULFF",
            "WIBLE",
            "WINER",
            "WIEST",
            "WALKO",
            "WALLA",
            "WEBRE",
            "WEYER",
            "WYBLE",
            "WOMAC",
            "WILTZ",
            "WURST",
            "WOLAK",
            "WELKE",
            "WEDEL",
            "WEIST",
            "WYGAN",
            "WUEST",
            "WEISZ",
            "WALCK",
            "WEITZ",
            "WYDRA",
            "WANDA",
            "WILMA",
            "WEBER",
            "WETZEL",
            "WEINER",
            "WENZEL",
            "WESTER",
            "WALLEN",
            "WENGER",
            "WALLIN",
            "WEILER",
            "WIMMER",
            "WEIMER",
            "WYRICK",
            "WEGNER",
            "WINNER",
            "WESSEL",
            "WILKIE",
            "WEIGEL",
            "WOJCIK",
            "WENDEL",
            "WITTER",
            "WIENER",
            "WEISER",
            "WEXLER",
            "WACKER",
            "WISNER",
            "WITMER",
            "WINKLE",
            "WELTER",
            "WIDMER",
            "WITTEN",
            "WINDLE",
            "WASHER",
            "WOLTER",
            "WILKEY",
            "WIDNER",
            "WARMAN",
            "WEYANT",
            "WEIBEL",
            "WANNER",
            "WILKEN",
            "WILTSE",
            "WARNKE",
            "WALSER",
            "WEIKEL",
            "WESNER",
            "WITZEL",
            "WROBEL",
            "WAGNON",
            "WINANS",
            "WENNER",
            "WOLKEN",
            "WILNER",
            "WYSONG",
            "WYCOFF",
            "WUNDER",
            "WINKEL",
            "WIDMAN",
            "WELSCH",
            "WEHNER",
            "WEIGLE",
            "WETTER",
            "WUNSCH",
            "WHITTY",
            "WAXMAN",
            "WILKER",
            "WILHAM",
            "WITTIG",
            "WITMAN",
            "WESTRA",
            "WEHRLE",
            "WASSER",
            "WILLER",
            "WEGMAN",
            "WARFEL",
            "WYNTER",
            "WERNER",
            "WAGNER",
            "WISSER",
            "WISEMAN",
            "WINKLER",
            "WILHELM",
            "WELLMAN",
            "WAMPLER",
            "WACHTER",
            "WALTHER",
            "WYCKOFF",
            "WEIDNER",
            "WOZNIAK",
            "WEILAND",
            "WILFONG",
            "WIEGAND",
            "WILCHER",
            "WIELAND",
            "WILDMAN",
            "WALDMAN",
            "WORTMAN",
            "WYSOCKI",
            "WEIDMAN",
            "WITTMAN",
            "WIDENER",
            "WOLFSON",
            "WENDELL",
            "WEITZEL",
            "WILLMAN",
            "WALDRUP",
            "WALTMAN",
            "WALCZAK",
            "WEIGAND",
            "WESSELS",
            "WIDEMAN",
            "WOLTERS",
            "WIREMAN",
            "WILHOIT",
            "WEGENER",
            "WOTRING",
            "WINGERT",
            "WIESNER",
            "WAYMIRE",
            "WHETZEL",
            "WENTZEL",
            "WINEGAR",
            "WESTMAN",
            "WYNKOOP",
            "WALLICK",
            "WURSTER",
            "WINBUSH",
            "WILBERT",
            "WALLACH",
            "WEISSER",
            "WEISNER",
            "WINDERS",
            "WILLMON",
            "WILLEMS",
            "WIERSMA",
            "WACHTEL",
            "WARNICK",
            "WEIDLER",
            "WALTRIP",
            "WHETSEL",
            "WHELESS",
            "WELCHER",
            "WALBORN",
            "WILLSEY",
            "WEINMAN",
            "WAGAMAN",
            "WOMMACK",
            "WINGLER",
            "WINKLES",
            "WIEDMAN",
            "WHITNER",
            "WOLFRAM",
            "WARLICK",
            "WEEDMAN",
            "WHISMAN",
            "WINLAND",
            "WEESNER",
            "WARTHEN",
            "WETZLER",
            "WENDLER",
            "WALLNER",
            "WOLBERT",
            "WITTMER",
            "WISHART",
            "WILLIAM",
            "WESTPHAL",
            "WICKLUND",
            "WEISSMAN",
            "WESTLUND",
            "WOLFGANG",
            "WILLHITE",
            "WEISBERG",
            "WALRAVEN",
            "WOLFGRAM",
            "WILHOITE",
            "WECHSLER",
            "WENDLING",
            "WESTBERG",
            "WENDLAND",
            "WININGER",
            "WHISNANT",
            "WESTRICK",
            "WESTLING",
            "WESTBURY",
            "WEITZMAN",
            "WEHMEYER",
            "WEINMANN",
            "WISNESKI",
            "WHELCHEL",
            "WEISHAAR",
            "WAGGENER",
            "WALDROUP",
            "WESTHOFF",
            "WIEDEMAN",
            "WASINGER",
            "WINBORNE",
            "WHISENANT",
            "WEINSTEIN",
            "WESTERMAN",
            "WASSERMAN",
            "WITKOWSKI",
            "WEINTRAUB",
            "WINKELMAN",
            "WINKFIELD",
            "WANAMAKER",
            "WIECZOREK",
            "WIECHMANN",
            "WOJTOWICZ",
            "WALKOWIAK",
            "WEINSTOCK",
            "WILLEFORD",
            "WARKENTIN",
            "WEISINGER",
            "WINKLEMAN",
            "WILHEMINA",
            "WISNIEWSKI",
            "WUNDERLICH",
            "WHISENHUNT",
            "WEINBERGER",
            "WROBLEWSKI",
            "WAGUESPACK",
            "WEISGERBER",
            "WESTERVELT",
            "WESTERLUND",
            "WASILEWSKI",
            "WILDERMUTH",
            "WESTENDORF",
            "WESOLOWSKI",
            "WEINGARTEN",
            "WINEBARGER",
            "WESTERBERG",
            "WANNAMAKER",
            "WEISSINGER",
            "WALDSCHMIDT",
            "WEINGARTNER",
            "WINEBRENNER",
            "WOLFENBARGER",
            "WOJCIECHOWSKI",
        ])
    }

    fn encode_x(&mut self) {
        if self.encode_initial_x()
            || self.encode_greek_x()
            || self.encode_x_special_cases()
            || self.encode_x_to_h()
            || self.encode_x_vowel()
            || self.encode_french_x_final()
        {
            return;
        }

        // eat redundant 'X' or other redundant cases
        // e.g. "excite", "exceed"
        if self.string_at(1, &["X", "Z", "S", "CI", "CE"]) {
            self.idx += 1;
        }
    }

    fn encode_initial_x(&mut self) -> bool {
        // current chinese pinyin spelling
        if self.string_start(&["XU", "XIA", "XIO", "XIE"]) {
            self.metaph_add('X');
            return true;
        }

        if self.idx == 0 {
            self.metaph_add('S');
            return true;
        }

        false
    }

    // 'xylophone', xylem', 'xanthoma', 'xeno-'
    fn encode_greek_x(&mut self) -> bool {
        if self.string_at(1, &["YLO", "YLE", "ENO", "ANTH"]) {
            self.metaph_add('S');
            return true;
        }
        false
    }

    //Encode special cases, "LUXUR-", "Texeira"
    fn encode_x_special_cases(&mut self) -> bool {
        if self.string_at(-2, &["LUXUR"]) {
            self.metaph_add_exact_approx_str("GJ", "KJ");
            return true;
        }

        if self.string_start(&["TEXEIRA", "TEIXEIRA"]) {
            self.metaph_add('X');
            return true;
        }
        false
    }

    //Encode special case where americans know the proper mexican indian
    //pronounciation of this name
    fn encode_x_to_h(&mut self) -> bool {
        if self.string_at(-2, &["OAXACA"]) || self.string_at(-3, &["QUIXOTE"]) {
            self.metaph_add('H');
            return true;
        }
        false
    }

    fn encode_x_vowel(&mut self) -> bool {
        // e.g. "sexual", "connexion" (british), "noxious"
        if self.string_at(1, &["UAL", "ION", "IOU"]) {
            self.metaph_add_str("KX", "KS");
            self.advance_counter(2, 0);
            return true;
        }
        false
    }

    fn encode_french_x_final(&mut self) -> bool {
        if !(self.idx == self.last_idx
            && (self.string_at(-3, &["IAU", "EAU", "IEU"])
                || self.string_at(-2, &["AI", "AU", "OU", "OI", "EU"])))
        {
            self.metaph_add_str("KS", "KS");
            //no return true?
        }
        false
    }

    fn encode_z(&mut self) {
        if self.encode_zz()
            || self.encode_zu_zier_zs()
            || self.encode_french_ez()
            || self.encode_german_z()
            || self.encode_zh()
        {
            return;
        }

        self.metaph_add('S');

        // eat redundant 'Z'
        if self.char_next_is('Z') {
            self.idx += 1;
        }
    }

    //Encode cases of "-ZZ-" where it is obviously part of an italian word where
    //"-ZZ-" is pronounced as TS
    fn encode_zz(&mut self) -> bool {
        // "abruzzi", 'pizza'
        if self.char_next_is('Z')
            && (self.string_at_end(2, &["I", "O", "A"])
                || self.string_at(-2, &["MOZZARELL", "PIZZICATO", "PUZZONLAN"]))
        {
            self.metaph_add_str("TS", "S");
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_zu_zier_zs(&mut self) -> bool {
        if (self.idx == 1 && self.string_at(-1, &["AZUR"]))
            || (self.string_at(0, &["ZIER"]) && !self.string_at(-2, &["VIZIER"]))
            || self.string_at(0, &["ZSA"])
        {
            self.metaph_add_alt('J', 'S');

            if self.string_at(0, &["ZSA"]) {
                self.idx += 1;
            }
            return true;
        }
        false
    }

    //Encode cases where americans recognize "-EZ" as part of a french word where Z
    //not pronounced
    fn encode_french_ez(&mut self) -> bool {
        if (self.idx == 3 && self.string_at(-3, &["CHEZ"])) || self.string_at(-5, &["RENDEZ"]) {
            return true;
        }

        false
    }

    //Encode cases where "-Z-" is in a german word where Z => TS in german
    fn encode_german_z(&mut self) -> bool {
        if self.string_exact(&["NAZI"])
            || self.string_at(-2, &["NAZIFY", "MOZART"])
            || self.string_at(-3, &["HOLZ", "HERZ", "MERZ", "FITZ", "HERZOG"])
            || (self.string_at(-3, &["GANZ"]) && !self.is_vowel_at(1))
            || self.string_at(-4, &["STOLZ", "PRINZ", "VENEZIA"])
            || // german words containing with "sch" but not schlimazel, schmooze
            (self.string_contains("SCH") && !self.string_end(&["IZE", "OZE", "ZEL"]))
            || (self.idx > 0 && self.string_at(0, &["ZEIT"]))
            || self.string_at(-3, &["WEIZ"])
        {
            if self.idx > 0 && self.char_at(-1, 'T') {
                self.metaph_add('S');
            } else {
                self.metaph_add_str("TS", "TS");
            }
            return true;
        }

        false
    }

    fn encode_zh(&mut self) -> bool {
        // chinese pinyin e.g. 'zhao', also english "phonetic spelling"
        if self.char_next_is('H') {
            self.metaph_add('J');
            self.idx += 1;
            return true;
        }
        false
    }

    fn encode_vowels(&mut self) {
        if self.idx == 0 {
            // all init vowels map to 'A'
            // as of Double Metaphone
            self.metaph_add('A');
        } else if self.encode_vowels {
            if !self.char_at(0, 'E') {
                if self.encode_skip_silent_ue() {
                    return;
                }
                if self.encode_o_silent() {
                    return;
                }
                // encode all vowels and
                // diphthongs to the same value
                self.metaph_add('A');
            } else {
                self.encode_e_pronounced();
            }
        }

        if !(!self.is_vowel_at(-2) && self.string_at(-1, &["LEWA", "LEWO", "LEWI"])) {
            self.idx = self.skip_vowels(self.idx + 1);
        }
    }

    fn encode_skip_silent_ue(&mut self) -> bool {
        // always silent except for cases listed below
        if (self.string_at(-1, &["QUE", "GUE"])
            && !self.string_start(&[
                "RISQUE",
                "PIROGUE",
                "ENRIQUE",
                "BARBEQUE",
                "PALENQUE",
                "APPLIQUE",
                "COMMUNIQUE",
            ])
            && !self.string_at(-3, &["ARGUE", "SEGUE"]))
            && self.idx > 1
            && ((self.idx + 1 == self.last_idx) || self.string_start(&["JACQUES"]))
        {
            self.idx = self.skip_vowels(self.idx);
            return true;
        }
        false
    }

    // Encodes cases where non-initial 'e' is pronounced, taking
    // care to detect unusual cases from the greek.
    // Only executed if non initial vowel encoding is turned on
    fn encode_e_pronounced(&mut self) {
        // special cases with two pronunciations
        // 'agape' 'lame' 'resume'
        if self.string_exact(&["LAME", "SAKE", "PATE", "AGAPE"])
            || (self.string_start(&["RESUME"]) && self.idx == 5)
        {
            self.metaph_add_alt('\0', 'A');
            return;
        }

        // special case "inge" => 'INGA', 'INJ'
        if self.string_exact(&["INGE"]) {
            self.metaph_add_alt('A', '\0');
            return;
        }

        // special cases with two pronunciations
        // special handling due to the difference in
        // the pronunciation of the '-D'
        if self.idx == 5 && self.string_start(&["BLESSED", "LEARNED"]) {
            self.metaph_add_exact_approx_alt("D", "AD", "T", "AT");
            self.idx += 1;
            return;
        }

        // encode all vowels and diphthongs to the same value
        if (!self.encode_e_silent() && !self.flag_al_inversion && !self.encode_silent_internal_e())
            || self.encode_e_pronounced_exceptions()
        {
            self.metaph_add('A');
        }

        // now that we've visited the vowel in question
        self.flag_al_inversion = false;
    }

    fn encode_o_silent(&mut self) -> bool {
        // if "iron" at beginning or end of word and not "irony"
        if self.char_at(0, 'O')
            && self.string_at(-2, &["IRON"])
            && (self.string_start(&["IRON"]) || self.string_at_end(-2, &["IRON"]))
            && !self.string_at(-2, &["IRONIC"])
        {
            return true;
        }

        false
    }

    fn encode_e_silent(&mut self) -> bool {
        if self.encode_e_pronounced_at_end() {
            return false;
        }

        // 'e' silent when last letter, altho
        if self.idx == self.last_idx ||
            // also silent if before plural 's'
            // or past tense or participle 'd', e.g.
            // 'grapes' and 'banished' => PNXT
            (self.idx > 1 && self.idx + 1 == self.last_idx && self.string_at(1, &["S", "D"]) &&
                // and not e.g. "nested", "rises", or "pieces" => RASAS
                !(self.string_at(-1, &["TED", "SES", "CES"]) ||
                    self.string_start(&["ABED", "IMED", "JARED", "AHMED", "HAMED", "JAVED",
                        "NORRED", "MEDVED", "MERCED", "ALLRED", "KHALED", "RASHED", "MASJED",
                        "MOHAMED", "MOHAMMED", "MUHAMMED", "MOUHAMED", "ANTIPODES", "ANOPHELES"]))) ||
            // e.g.  'wholeness', 'boneless', 'barely'
            self.string_at_end(1, &["NESS", "LESS"]) ||
            (self.string_at_end(1, &["LY"]) && !self.string_start(&["CICELY"]))
        {
            return true;
        }
        false
    }

    // Tests for words where an 'E' at the end of the word
    // is pronounced
    //
    // special cases, mostly from the greek, spanish, japanese,
    // italian, and french words normally having an acute accent.
    // also, pronouns and articles
    //
    // Many Thanks to ali, QuentinCompson, JeffCO, ToonScribe, Xan,
    // Trafalz, and VictorLaszlo, all of them atriots from the Eschaton,
    // for all their fine contributions!
    fn encode_e_pronounced_at_end(&mut self) -> bool {
        if self.idx == self.last_idx
            && (self.string_at(-6, &["STROPHE"]) ||
                // if a vowel is before the 'E', vowel eater will have eaten it.
                //otherwise, consonant + 'E' will need 'E' pronounced
                self.in_buf.len() == 2 ||
                (self.in_buf.len() == 3 && !self.is_vowel_at(-(self.idx as isize))) ||
                // these german name endings can be relied on to have the 'e' pronounced
                (self.string_at_end(-2, &["BKE", "DKE", "FKE", "KKE", "LKE", "NKE", "MKE", "PKE", "TKE", "VKE", "ZKE"]) &&
                    !self.string_start(&["FINKE", "FUNKE", "FRANKE"])) ||
                self.string_at_end(-4, &["SCHKE"]) ||
                self.string_exact(&["ACME", "NIKE", "CAFE", "RENE", "LUPE", "JOSE", "ESME",
                    "LETHE", "CADRE", "TILDE", "SIGNE", "POSSE", "LATTE", "ANIME", "DOLCE", "CROCE",
                    "ADOBE", "OUTRE", "JESSE", "JAIME", "JAFFE", "BENGE", "RUNGE",
                    "CHILE", "DESME", "CONDE", "URIBE", "LIBRE", "ANDRE",
                    "HECATE", "PSYCHE", "DAPHNE", "PENSKE", "CLICHE", "RECIPE",
                    "TAMALE", "SESAME", "SIMILE", "FINALE", "KARATE", "RENATE", "SHANTE",
                    "OBERLE", "COYOTE", "KRESGE", "STONGE", "STANGE", "SWAYZE", "FUENTE",
                    "SALOME", "URRIBE",
                    "ECHIDNE", "ARIADNE", "MEINEKE", "PORSCHE", "ANEMONE", "EPITOME",
                    "SYNCOPE", "SOUFFLE", "ATTACHE", "MACHETE", "KARAOKE", "BUKKAKE",
                    "VICENTE", "ELLERBE", "VERSACE",
                    "PENELOPE", "CALLIOPE", "CHIPOTLE", "ANTIGONE", "KAMIKAZE", "EURIDICE",
                    "YOSEMITE", "FERRANTE",
                    "HYPERBOLE", "GUACAMOLE", "XANTHIPPE",
                    "SYNECDOCHE"]))
        {
            return true;
        }

        false
    }

    fn encode_silent_internal_e(&mut self) -> bool {
        // 'olesen' but not 'olen'	RAKE BLAKE
        if (self.string_start(&["OLE"]) && self.encode_e_suffix(3))
            || (self.string_start(&[
                "BARE", "FIRE", "FORE", "GATE", "HAGE", "HAVE", "HAZE", "HOLE", "CAPE", "HUSE",
                "LACE", "LINE", "LIVE", "LOVE", "MORE", "MOSE", "MORE", "NICE", "RAKE", "ROBE",
                "ROSE", "SISE", "SIZE", "WARE", "WAKE", "WISE", "WINE",
            ]) && self.encode_e_suffix(4))
            || (self.string_start(&[
                "BLAKE", "BRAKE", "BRINE", "CARLE", "CLEVE", "DUNNE", "HEDGE", "HOUSE", "JEFFE",
                "LUNCE", "STOKE", "STONE", "THORE", "WEDGE", "WHITE",
            ]) && self.encode_e_suffix(5))
            || (self.string_start(&["BRIDGE", "CHEESE"]) && self.encode_e_suffix(6))
            || (self.string_at(-5, &["CHARLES"]))
        {
            return true;
        }

        false
    }

    fn encode_e_suffix(&mut self, at: usize) -> bool {
        //E_Silent_Suffix && !E_Pronouncing_Suffix

        if self.idx == at - 1
            && self.in_buf.len() > at + 1
            && (self.is_vowel_at(-((self.idx as isize) - (at as isize) - 1))
                || (self.string_at(-((self.idx as isize) - (at as isize)), &["ST", "SL"])
                    && self.in_buf.len() > at + 2))
        {
            // now filter endings that will cause the 'e' to be pronounced

            // e.g. 'bridgewood' - the other vowels will get eaten
            // up so we need to put one in here
            // e.g. 'bridgette'
            // e.g. 'olena'
            // e.g. 'bridget'
            if self.string_at_end(
                -((self.idx as isize) - (at as isize)),
                &[
                    "T", "R", "TA", "TT", "NA", "NO", "NE", "RS", "RE", "LA", "AU", "RO", "RA",
                    "TTE", "LIA", "NOW", "ROS", "RAS", "WOOD", "WATER", "WORTH",
                ],
            ) {
                return false;
            }

            return true;
        }

        false
    }

    // Exceptions where 'E' is pronounced where it
    // usually wouldn't be, and also some cases
    // where 'LE' transposition rules don't apply
    // and the vowel needs to be encoded here
    fn encode_e_pronounced_exceptions(&mut self) -> bool {
        // greek names e.g. "herakles" or hispanic names e.g. "robles", where 'e' is pronounced, other exceptions
        if (self.idx + 1 == self.last_idx
            && (self.string_at_end(-3, &["OCLES", "ACLES", "AKLES"])
                || self.string_start(&[
                    "INES",
                    "LOPES",
                    "ESTES",
                    "GOMES",
                    "NUNES",
                    "ALVES",
                    "ICKES",
                    "INNES",
                    "PERES",
                    "WAGES",
                    "NEVES",
                    "BENES",
                    "DONES",
                    "CORTES",
                    "CHAVES",
                    "VALDES",
                    "ROBLES",
                    "TORRES",
                    "FLORES",
                    "BORGES",
                    "NIEVES",
                    "MONTES",
                    "SOARES",
                    "VALLES",
                    "GEDDES",
                    "ANDRES",
                    "VIAJES",
                    "CALLES",
                    "FONTES",
                    "HERMES",
                    "ACEVES",
                    "BATRES",
                    "MATHES",
                    "DELORES",
                    "MORALES",
                    "DOLORES",
                    "ANGELES",
                    "ROSALES",
                    "MIRELES",
                    "LINARES",
                    "PERALES",
                    "PAREDES",
                    "BRIONES",
                    "SANCHES",
                    "CAZARES",
                    "REVELES",
                    "ESTEVES",
                    "ALVARES",
                    "MATTHES",
                    "SOLARES",
                    "CASARES",
                    "CACERES",
                    "STURGES",
                    "RAMIRES",
                    "FUNCHES",
                    "BENITES",
                    "FUENTES",
                    "PUENTES",
                    "TABARES",
                    "HENTGES",
                    "VALORES",
                    "GONZALES",
                    "MERCEDES",
                    "FAGUNDES",
                    "JOHANNES",
                    "GONSALES",
                    "BERMUDES",
                    "CESPEDES",
                    "BETANCES",
                    "TERRONES",
                    "DIOGENES",
                    "CORRALES",
                    "CABRALES",
                    "MARTINES",
                    "GRAJALES",
                    "CERVANTES",
                    "FERNANDES",
                    "GONCALVES",
                    "BENEVIDES",
                    "CIFUENTES",
                    "SIFUENTES",
                    "SERVANTES",
                    "HERNANDES",
                    "BENAVIDES",
                    "ARCHIMEDES",
                    "CARRIZALES",
                    "MAGALLANES",
                ])))
            || self.string_at(-2, &["FRED", "DGES", "DRED", "GNES"])
            || self.string_at(-5, &["PROBLEM", "RESPLEN"])
            || self.string_at(-4, &["REPLEN"])
            || self.string_at(-3, &["SPLE"])
        {
            return true;
        }

        false
    }

    // ==============================================================================================
    // Helper Methods - ported from Go
    // ==============================================================================================

    /// Check if character at current idx + offset is equal to c
    fn char_at(&self, offset: isize, c: char) -> bool {
        let idx = self.idx as isize + offset;
        if idx < 0 || idx >= self.length as isize {
            return false;
        }
        self.in_buf[idx as usize] == c
    }

    /// Convenience method to check next character
    fn char_next_is(&self, c: char) -> bool {
        self.char_at(1, c)
    }

    /// Check if character at idx + offset is a vowel
    fn is_vowel_at(&self, offset: isize) -> bool {
        let idx = self.idx as isize + offset;
        if idx < 0 || idx >= self.length as isize {
            return false;
        }
        Self::is_vowel_char(self.in_buf[idx as usize])
    }

    /// Check if a character is a vowel
    fn is_vowel_char(c: char) -> bool {
        // OxRefine: Metaphone 3's own IsVowel (as OpenRefine runs it) counts vowels with diacritics
        matches!(
            c,
            'A' | 'E' | 'I' | 'O' | 'U' | 'Y'
                | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Æ'
                | 'È' | 'É' | 'Ê' | 'Ë'
                | 'Ì' | 'Í' | 'Î' | 'Ï'
                | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Œ' | 'Ø'
                | 'Ù' | 'Ú' | 'Û' | 'Ü'
                | 'Ý' | 'Ÿ'
        )
    }

    /// Returns true if `buf` starts with the characters of `s` (allocation-free).
    fn buf_starts_with(buf: &[char], s: &str) -> bool {
        let mut i = 0;
        for c in s.chars() {
            match buf.get(i) {
                Some(&b) if b == c => i += 1,
                _ => return false,
            }
        }
        true
    }

    /// Returns true if `buf` equals the characters of `s` exactly (allocation-free).
    fn buf_eq_str(buf: &[char], s: &str) -> bool {
        let mut chars = s.chars();
        for &b in buf {
            match chars.next() {
                Some(c) if c == b => {}
                _ => return false,
            }
        }
        chars.next().is_none()
    }

    /// Returns true if one of the given substrings is located at the
    /// relative offset (relative to current idx) given
    fn string_at(&self, offset: isize, vals: &[&str]) -> bool {
        let start = self.idx as isize + offset;

        // Basic bounds check
        if start < 0 || start >= self.length as isize {
            return false;
        }

        // Check if shortest val would overrun
        if vals.is_empty() || start as usize + vals[0].len() > self.length {
            return false;
        }

        let start = start as usize;

        for &val in vals {
            // Bounds check for this value
            if start + val.len() > self.length {
                return false;
            }

            // Compare directly against the input buffer without allocating.
            if Self::buf_starts_with(&self.in_buf[start..], val) {
                return true;
            }
        }

        false
    }

    /// Returns true if we're at the start of the string and it starts with one of the given vals
    fn string_at_start(&self, offset: isize, vals: &[&str]) -> bool {
        if offset != -(self.idx as isize) {
            return false;
        }
        self.string_at(offset, vals)
    }

    /// Returns true if one of the given substrings is located at the relative offset
    /// and uses all the remaining letters of the input
    fn string_at_end(&self, offset: isize, vals: &[&str]) -> bool {
        let start = self.idx as isize + offset;

        if start < 0 || start >= self.length as isize {
            return false;
        }

        if vals.is_empty() || start as usize + vals[0].len() > self.length {
            return false;
        }

        let start = start as usize;

        for &val in vals {
            if Self::buf_eq_str(&self.in_buf[start..], val) {
                return true;
            }
        }

        false
    }

    /// Check if entire string starts with one of the given values
    fn string_start(&self, vals: &[&str]) -> bool {
        self.string_at(-(self.idx as isize), vals)
    }

    /// Check if entire string ends with one of the given values (regardless of current position)
    fn string_end(&self, vals: &[&str]) -> bool {
        for &val in vals {
            let val_len = val.len();
            if val_len > self.length {
                return false;
            }

            let start = self.length - val_len;
            if Self::buf_eq_str(&self.in_buf[start..], val) {
                return true;
            }
        }
        false
    }

    /// Check if entire string exactly matches one of the given values
    fn string_exact(&self, vals: &[&str]) -> bool {
        for &val in vals {
            if val.len() != self.length {
                continue;
            }

            if Self::buf_eq_str(&self.in_buf, val) {
                return true;
            }
        }
        false
    }

    /// Check if string contains the given value anywhere
    fn string_contains(&self, val: &str) -> bool {
        let val_len = val.chars().count();

        if val_len > self.length {
            return false;
        }

        for i in 0..=(self.length - val_len) {
            if Self::buf_starts_with(&self.in_buf[i..], val) {
                return true;
            }
        }
        false
    }

    // ==============================================================================================
    // Output manipulation methods
    // ==============================================================================================

    /// Adds encoding character to both primary and secondary buffers
    fn metaph_add(&mut self, c: char) {
        self.metaph_add_alt(c, c);
    }

    /// Adds different encoding characters to primary and secondary buffers
    fn metaph_add_alt(&mut self, prim: char, second: char) {
        // Add to primary buffer if not null (don't duplicate A's)
        if prim != '\0'
            && !(prim == 'A'
                && !self.prim_buf.is_empty()
                && self.prim_buf[self.prim_buf.len() - 1] == 'A')
        {
            self.prim_buf.push(prim);
        }

        // Add to secondary buffer if not null (don't duplicate A's)
        if second != '\0'
            && !(second == 'A'
                && !self.second_buf.is_empty()
                && self.second_buf[self.second_buf.len() - 1] == 'A')
        {
            self.second_buf.push(second);
        }
    }

    /// Adds strings to both buffers
    fn metaph_add_str(&mut self, prim: &str, second: &str) {
        // Add primary string (don't duplicate A's)
        if !(prim == "A"
            && !self.prim_buf.is_empty()
            && self.prim_buf[self.prim_buf.len() - 1] == 'A')
        {
            for c in prim.chars() {
                self.prim_buf.push(c);
            }
        }

        // Add secondary string (don't duplicate A's)
        if !second.is_empty()
            && !(second == "A"
                && !self.second_buf.is_empty()
                && self.second_buf[self.second_buf.len() - 1] == 'A')
        {
            for c in second.chars() {
                self.second_buf.push(c);
            }
        }
    }

    /// Adds exact or approximate encoding based on the `encode_exact` setting
    fn metaph_add_exact_approx(&mut self, exact: char, approx: char) {
        if self.encode_exact {
            self.metaph_add(exact);
        } else {
            self.metaph_add(approx);
        }
    }

    /// String version of `metaph_add_exact_approx`
    fn metaph_add_exact_approx_str(&mut self, exact: &str, approx: &str) {
        if self.encode_exact {
            self.metaph_add_str(exact, exact);
        } else {
            self.metaph_add_str(approx, approx);
        }
    }

    /// Adds exact or approximate encodings with alternates
    fn metaph_add_exact_approx_alt(&mut self, exact: &str, alt_exact: &str, main: &str, alt: &str) {
        if self.encode_exact {
            self.metaph_add_str(exact, alt_exact);
        } else {
            self.metaph_add_str(main, alt);
        }
    }

    /// Skip vowels from the given position, returning the position after vowels
    fn skip_vowels(&self, at: usize) -> usize {
        if at >= self.length {
            return self.length - 1;
        }

        let mut pos = at;
        let mut c = self.in_buf[pos];

        while (Self::is_vowel_char(c) || c == 'W') && pos < self.length {
            let off = pos as isize - self.idx as isize;

            // Check for Polish/Slavic endings
            if self.string_at(off, &["WICZ", "WITZ", "WIAK"])
                || self.string_at(off - 1, &["EWSKI", "EWSKY", "OWSKI", "OWSKY"])
                || self.string_at_end(off, &["WICKI", "WACKI"])
            {
                break;
            }

            pos += 1;

            // Check for WH combinations
            if pos >= 2
                && self.in_buf[pos - 1] == 'W'
                && pos < self.length
                && self.in_buf[pos] == 'H'
            {
                let off2 = pos as isize - self.idx as isize;
                if !self.string_at(
                    off2,
                    &[
                        "HOP", "HIDE", "HARD", "HEAD", "HAWK", "HERD", "HOOK", "HAND", "HOLE",
                        "HEART", "HOUSE", "HOUND", "HAMMER",
                    ],
                ) {
                    pos += 1;
                }
            }

            if pos >= self.length {
                break;
            }

            c = self.in_buf[pos];
        }

        if pos > 0 { pos - 1 } else { 0 }
    }

    /// Advances the counter conditionally based on the `encode_vowels` setting
    fn advance_counter(&mut self, no_encode_vowel: usize, encode_vowel: usize) {
        if self.encode_vowels {
            self.idx += encode_vowel;
        } else {
            self.idx += no_encode_vowel;
        }
    }

    /// Check if the word looks Slavic or Germanic
    fn is_slavo_germanic(&self) -> bool {
        if self.length == 0 {
            return false;
        }

        let first = self.in_buf[0];
        if first == 'J' || first == 'W' {
            return true;
        }

        self.string_start(&["SCH", "SW"])
    }

    /// Check if the input word is the root itself or a common inflection of the root
    fn root_or_inflections(&self, root: &str) -> bool {
        Self::root_or_inflections_slice(&self.in_buf, root)
    }

    fn root_or_inflections_from(&self, from: usize, root: &str) -> bool {
        if from >= self.in_buf.len() {
            return false;
        }
        Self::root_or_inflections_slice(&self.in_buf[from..], root)
    }

    fn root_or_inflections_slice(in_word: &[char], root: &str) -> bool {
        let root_chars: Vec<char> = root.chars().collect();
        let len_diff = in_word.len() as isize - root_chars.len() as isize;

        // there's no alternate shorter than the root itself
        if len_diff < 0 {
            return false;
        }

        // inWord must start with all the letters of root except the last
        let last = root_chars.len() - 1;
        for i in 0..last {
            if in_word[i] != root_chars[i] {
                return false;
            }
        }

        let in_word = &in_word[last..];
        // at this point we know they start the same way
        // except the last rune of root that we didn't check, so check that now
        // check our last letter and simple plural

        if in_word[0] == root_chars[last] {
            // last root letter matches
            if len_diff == 0 {
                //exact match
                return true;
            } else if len_diff == 1 && in_word.len() > 1 && in_word[1] == 'S' {
                // match with an extra S
                return true;
            }
        }

        // different paths if the last letter is 'E' or not
        let mut len_diff = len_diff;
        let in_word = if root_chars[last] == 'E' {
            // check ED
            if len_diff == 1 && in_word.len() >= 2 && in_word[0] == 'E' && in_word[1] == 'D' {
                return true;
            }
            // we now consider the 'E' to be a difference
            len_diff += 1;
            in_word
        } else {
            // check +ES
            // check +ED
            // the last character must match if the root doesn't end in E
            if in_word[0] != root_chars[last] {
                return false;
            }

            if len_diff == 2
                && in_word.len() >= 3
                && in_word[1] == 'E'
                && (in_word[2] == 'S' || in_word[2] == 'D')
            {
                return true;
            }

            //we know the last letter matches, so chop it off
            &in_word[1..]
        };

        // at this point our root and inWord match, so now we're just checking the endings
        // of the inWord starting at index "last"

        if len_diff == 3 && in_word.len() >= 3 && in_word[..3] == ['I', 'N', 'G'] {
            // check ING
            return true;
        } else if len_diff == 5 && in_word.len() >= 5 && in_word[..5] == ['I', 'N', 'G', 'L', 'Y'] {
            // check INGLY
            return true;
        } else if len_diff == 1 && !in_word.is_empty() && in_word[0] == 'Y' {
            // check Y
            return true;
        }

        false
    }
}

// Default implementation for convenience
impl Default for Metaphone3 {
    fn default() -> Self {
        Self::new()
    }
}
