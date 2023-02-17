// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//! GOTO binary serializer.

use crate::irep::{Irep, IrepId, Symbol, SymbolTable};
use crate::{InternString, InternedString};
use std::collections::HashMap;
use std::fs::File;
use std::hash::Hash;
use std::io::{self, BufReader};
use std::io::{BufWriter, Bytes, Error, ErrorKind, Read, Write};
use std::path::PathBuf;

/// Writes a symbol table to a file in goto binary format in version 5.
///
/// In CBMC, the serialization rules are defined in :
/// - src/goto-programs/write_goto_binary.h
/// - src/util/irep_serialization.h
/// - src/util/irep_hash_container.h
/// - src/util/irep_hash.h
pub fn write_goto_binary_file(filename: &PathBuf, source: &crate::goto_program::SymbolTable) {
    let out_file = File::create(filename).unwrap();
    let mut writer = BufWriter::new(out_file);
    let mut serializer = GotoBinarySerializer::new(&mut writer);
    let irep_symbol_table = &source.to_irep();
    serializer.write_file(irep_symbol_table).unwrap();
}

/// Reads a symbol table from a file expected to be in goto binary format in version 5.
//
/// In CBMC, the deserialization rules are defined in :
/// - src/goto-programs/read_goto_binary.h
/// - src/util/irep_serialization.h
/// - src/util/irep_hash_container.h
/// - src/util/irep_hash.h
pub fn read_goto_binary_file(filename: &PathBuf) {
    let file = File::open(filename).unwrap();
    let reader = BufReader::new(file);
    let mut deserializer = GotoBinaryDeserializer::new(reader);
    deserializer.read_file().unwrap();
}

/// # Design overview
///
/// When saving a [SymbolTable] to a binary file, the [Irep] describing each
/// symbol's type, value and source location are structurally hashed and
/// uniquely numbered so that structurally identical [Irep] only get written
/// in full to the file the first time they are encountered and that ulterior
/// occurrences are referenced by their unique number instead.
/// The key concept at play is that of a numbering, ie a function that assigns
/// numbers to values of a given type.
///
/// The [IrepNumbering] struct offers high-level methods to number
/// [InternedString], [IrepId] and [Irep] values:
/// - [InternedString] objects get mapped to [NumberedString] objects based on
///   the characters they contain.
/// - [IrepId] objects get mapped to [NumberedString] objects based on the
///   characters of their string representation, in the same number space
///   as [InternedString].
/// - [Irep] objects get mapped to [NumberedIrep] based on:
///     + the unique numbers assigned to their [Irep::id] attribute,
///     + the unique numbers of [Irep] found in their [IrepId::sub] attribute,
///     + the pairs of unique numbers assigned to the ([IrepId],[Irep]) pairs
///       found in their [IpreId::named_sub] attribute.
///
/// In order to assign the same number to structurally identical [Irep] objects,
/// [IrepNumbering] essentially implements a cache where each [NumberedIrep] is
/// keyed under an [IrepKey] that describes its internal structure.
///
/// An [IrepKey] is simply the vector of unique numbers describing the
/// `id`, `sub` and `named_sub` attributes of a [Irep].
///
/// A [NumberedIrep] is conceptually a pair made of the [IrepKey] itself and the
/// unique number assigned to that key.
///
/// The cache implemented by [IrepNumbering] is bidirectional. It lets you
/// compute the [NumberedIrep] of an [Irep], and lets you fetch a numbered
/// [NumberedIrep] from its unique number.
///
/// In practice:
/// - the forward directon from [IrepKey] to unique numbers is
/// implemented using a `HashMap<IrepKey,usize>`
/// - the inverse direction from unique numbers to [NumberedIrep] is implemented
/// using a `Vec<NumberedIrep>` called the `index` that stores [NumberedIrep]
/// under their unique number.
///
/// Earlier we said that an [NumberedIrep] is conceptually a pair formed of
/// an [IrepKey] and its unique number. In practice, is  represented using only
/// a pair formed of a `usize` representing the unique number, and a `usize`
/// representing the index at which the key can be found inside a single vector
/// of type `Vec<usize>` called `keys` where all [IrepKey] are concatenated when
/// they first get numbered. The inverse map of keys is represented this way
/// because the Rust hash map that implements the forward cache owns
/// its keys which would make it difficult to store keys references in inverse
/// cache, which would introduce circular dependencies and require `Rc` and
/// liftetimes annotations everywhere.
///
/// Numberig an [Irep] consists in recursively traversing it and numbering its
/// contents in a bottom-up fashion, then assembling its [IrepKey] and querying
/// the cache to either return an existing [NumberedIrep] or creating a new one
/// (in case that key has never been seen before).
///
/// The [GotoBinarySerializer] internally uses a [IrepNumbering] and a cache
/// of [NumberedIrep] and [NumberedString] it has already written to file.
///
/// When given an [InternedString], [IrepId] or [Irep] to serialize,
/// the [GotoBinarySerializer] first numbers that object using its internal
/// [IrepNumbering] instance. Then it looks up that unique number in its cache
/// of already written objects. If the object was seen before, only the unique
/// number of that object is written to file. If the object was never seen
/// before, then the unique number of that object is written to file, followed
/// by the objects describing its contents (themselves only being written fully
/// if they have never been seen before, or only referenced if they have been
/// seen before, etc.)
///
/// The [GotoBinaryDeserializer] also uses an [IrepNumbering] and a cache
/// of [NumberedIrep] and [NumberedString] it has already read from file.
/// Dually to the serializer, it will only attempt to decode the contents of an
/// object from the byte stream on the first occurrence.

/// A numbered [InternedString]. The number is guaranteed to be in [0,N].
/// Had to introduce this indirection because [InternedString] does not let you access
/// its unique id, so we have to build one ourselves.
#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
struct NumberedString {
    number: usize,
    string: InternedString,
}

/// An [Irep] represented by the vector of unique numbers of its contents.
#[derive(PartialEq, Eq, Hash, Clone, Debug)]
struct IrepKey {
    numbers: Vec<usize>,
}

impl IrepKey {
    /// Packs an [Irep]'s contents unique numbers into a new key object:
    /// - `id` must be the unique number assigned to an [Irep]'s [Irep::id] field.
    /// - `sub` must be the vector of unique number assigned to an [Irep]'s [Irep::sub] field.
    /// - `named_sub` must be the vector of unique number assigned to an [Irep]'s [Irep::named_sub] field.
    ///
    /// The `id`, `sub` and `named_sub` passed as arguments are packed as follows in the key's `number` field:
    /// ```
    /// id
    /// sub.len()
    /// sub[0]
    /// ...
    /// sub[sub.len()-1]
    /// named_sub.len()
    /// named_sub[0].0
    /// named_sub[0].1
    /// ...
    /// named_sub[named_sub.len()-1].0
    /// named_sub[named_sub.len()-1].1
    /// ```
    fn new(id: usize, sub: &[usize], named_sub: &[(usize, usize)]) -> Self {
        let size = sub.len() + 2 * named_sub.len() + 3;
        let mut vec: Vec<usize> = Vec::with_capacity(size);
        vec.push(id);
        vec.push(sub.len());
        vec.extend_from_slice(sub);
        vec.push(named_sub.len());
        for (k, v) in named_sub {
            vec.push(*k);
            vec.push(*v);
        }
        IrepKey { numbers: vec }
    }
}

/// Inverse cache of unique [NumberedIrep] objects.
struct IrepNumberingInv {
    /// Maps [Irep] numbers to [NumberedIrep]s;
    index: Vec<NumberedIrep>,

    /// Stores the concactenation of all [IrepKey] seen by the [IrepNumbering] object owning this inverse numbering.
    keys: Vec<usize>,
}

impl IrepNumberingInv {
    fn new() -> Self {
        IrepNumberingInv { index: Vec::new(), keys: Vec::new() }
    }

    /// Adds a key to the mapping and returns the unique number assigned to that key.
    fn add_key(&mut self, key: &IrepKey) -> usize {
        let number = self.index.len();
        self.index.push(NumberedIrep { number, start_index: self.keys.len() });
        self.keys.extend(&key.numbers);
        number
    }

    /// Returns a NumberedIrep from its unique number if it exists, None otherwise.
    fn numbered_irep_from_number(&self, irep_number: usize) -> Option<NumberedIrep> {
        self.index.get(irep_number).copied()
    }
}

/// A numbering of [InternedString], [IrepId] and [Irep] based on their contents.
struct IrepNumbering {
    /// Map from [InternedString] to their unique numbers.
    string_cache: HashMap<InternedString, usize>,

    /// Inverse string cache.
    inv_string_cache: Vec<NumberedString>,

    /// Map from [IrepKey] to their unique numbers.
    cache: HashMap<IrepKey, usize>,

    /// Inverse cache, allows to get a NumberedIrep from its unique number.
    inv_cache: IrepNumberingInv,
}

impl IrepNumbering {
    fn new() -> Self {
        IrepNumbering {
            string_cache: HashMap::new(),
            inv_string_cache: Vec::new(),
            cache: HashMap::new(),
            inv_cache: IrepNumberingInv::new(),
        }
    }

    /// Returns a [NumberedString] from its number if it exists, None otherwise.
    fn numbered_string_from_number(&mut self, string_number: usize) -> Option<NumberedString> {
        self.inv_string_cache.get(string_number).copied()
    }

    /// Returns a [NumberedIrep] from its number if it exists, None otherwise.
    fn numbered_irep_from_number(&mut self, irep_number: usize) -> Option<NumberedIrep> {
        self.inv_cache.numbered_irep_from_number(irep_number)
    }

    /// Turns a [InternedString] into a [NumberedString].
    fn number_string(&mut self, string: &InternedString) -> NumberedString {
        let len = self.string_cache.len();
        let entry = self.string_cache.entry(*string);
        let number = *entry.or_insert_with(|| {
            self.inv_string_cache.push(NumberedString { number: len, string: *string });
            len
        });
        self.inv_string_cache[number]
    }

    /// Turns a [IrepId] to a [NumberedString]. The [IrepId] gets the number of its
    /// string representation.
    fn number_irep_id(&mut self, irep_id: &IrepId) -> NumberedString {
        self.number_string(&irep_id.to_string().intern())
    }

    /// Turns an [Irep] into a [NumberedIrep]. The [Irep] is recursively traversed
    /// and numbered in a bottom-up fashion. Structurally identical [Irep]s
    /// result in the same [NumberedIrep].
    fn number_irep(&mut self, irep: &Irep) -> NumberedIrep {
        // build the key
        let id = self.number_irep_id(&irep.id).number;
        let sub: Vec<usize> = irep.sub.iter().map(|sub| self.number_irep(sub).number).collect();
        let named_sub: Vec<(usize, usize)> = irep
            .named_sub
            .iter()
            .map(|(key, value)| (self.number_irep_id(key).number, self.number_irep(value).number))
            .collect();
        let key = IrepKey::new(id, &sub, &named_sub);
        self.get_or_insert(&key)
    }

    /// Gets the existing [NumberedIrep] from the [IrepKey] or inserts a fresh
    /// one and returns it.
    fn get_or_insert(&mut self, key: &IrepKey) -> NumberedIrep {
        if let Some(number) = self.cache.get(key) {
            // Return the NumberedIrep from the inverse cache
            return self.inv_cache.index[*number];
        }
        // This is where the key gets its unique number assigneds.
        let number = self.inv_cache.add_key(&key);
        self.cache.insert(key.clone(), number);
        self.inv_cache.index[number]
    }

    /// Returns the unique number of the `id` field of the given [NumberedIrep].
    fn id(&self, numbered_irep: &NumberedIrep) -> NumberedString {
        self.inv_string_cache[self.inv_cache.keys[numbered_irep.start_index]]
    }

    /// Returns `#sub`, the number of `sub` [Irep]s of the given [NumberedIrep].
    /// It is found at `numbered_irep.start_index + 1` in the inverse cache.
    fn nof_sub(&self, numbered_irep: &NumberedIrep) -> usize {
        self.inv_cache.keys[numbered_irep.start_index + 1]
    }

    /// Returns the [NumberedIrep] for the ith `sub` of the given [NumberedIrep].
    fn sub(&self, numbered_irep: &NumberedIrep, sub_idx: usize) -> NumberedIrep {
        let sub_number = self.inv_cache.keys[numbered_irep.start_index + sub_idx + 2];
        self.inv_cache.index[sub_number]
    }

    /// Returns `#named_sub`, the number of named subs of the given [NumberedIrep].
    /// It is found at `numbered_irep.start_index + #sub + 2` in the inverse cache.
    fn nof_named_sub(&self, numbered_irep: &NumberedIrep) -> usize {
        self.inv_cache.keys[numbered_irep.start_index + self.nof_sub(numbered_irep) + 2]
    }

    /// Returns the pair of [NumberedString] and [NumberedIrep] for the named
    /// sub number `i` of this [NumberedIrep].
    fn named_sub(
        &self,
        numbered_irep: &NumberedIrep,
        named_sub_idx: usize,
    ) -> (NumberedString, NumberedIrep) {
        let start_index =
            numbered_irep.start_index + self.nof_sub(numbered_irep) + 2 * named_sub_idx + 3;
        (
            self.inv_string_cache[self.inv_cache.keys[start_index]],
            self.inv_cache.index[self.inv_cache.keys[start_index + 1]],
        )
    }
}

/// A uniquely numbered [Irep].
/// A NumberedIrep can be viewed as a generational index into a [IrepNumbering] instance.
#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
struct NumberedIrep {
    /// The unique number of this NumberedIrep.
    number: usize,
    /// Start index of the [IrepKey] of this [NumberedIrep] in the inverse cache of the [IrepNumbering] that produced it.
    start_index: usize,
}

/// GOTO binary serializer.
struct GotoBinarySerializer<'a, W>
where
    W: Write,
{
    writer: &'a mut W,

    /// In-memory temporary buffer, contents get flushed after each object
    buf: Vec<u8>,

    /// Numbering used for structural sharing.
    numbering: IrepNumbering,

    /// Counts how many times a given irep was written.
    irep_count: Vec<usize>,

    /// Counts how many times a given string was written.
    string_count: Vec<usize>,
}

impl<'a, W> GotoBinarySerializer<'a, W>
where
    W: Write,
{
    /// Constructor.
    fn new(writer: &'a mut W) -> Self {
        GotoBinarySerializer {
            writer,
            buf: Vec::new(),
            numbering: IrepNumbering::new(),
            irep_count: Vec::new(),
            string_count: Vec::new(),
        }
    }

    #[cfg(test)]
    /// Returns memory consumption and sharing statistics about the serializer.
    fn get_stats(&self) -> GotoBinarySharingStats {
        GotoBinarySharingStats::from_serializer(self)
    }

    /// Adds an InternedString uid to the "written" cache, returns true iff was never written before.
    fn is_first_write_string(&mut self, u: usize) -> bool {
        if u >= self.string_count.len() {
            self.string_count.resize(u + 1, 0);
        }
        let count = self.string_count[u];
        self.string_count[u] = count.checked_add(1).unwrap();
        count == 0
    }

    /// Adds an Irep uid to the "written" cache, returns true iff it was never written before.
    fn is_first_write_irep(&mut self, u: usize) -> bool {
        if u >= self.irep_count.len() {
            self.irep_count.resize(u + 1, 0);
        }
        let count = self.irep_count[u];
        self.irep_count[u] = count.checked_add(1).unwrap();
        count == 0
    }

    /// Flushes the temporary buffer to the external writer,
    /// flushes the writer and clears the temporary buffer.
    fn flush(&mut self) -> io::Result<()> {
        self.writer.write_all(&self.buf)?;
        self.buf.clear();
        Ok(())
    }

    /// Writes a single byte to the temporary buffer.
    fn write_u8(&mut self, u: u8) -> io::Result<()> {
        self.buf.push(u);
        Ok(())
    }

    /// Writes a usize to the temporary buffer using 7-bit variable length encoding.
    /// A usize value gets serialized as a list of u8. The usize value get shifted right in place, 7 bits at a time, the shifted
    /// bits are stored in the LSBs of a u8. The MSB of the u8 is used to indicate the continuation or the end of the encoding:
    /// - it is set to true if some true bits remain in the usize value,
    /// - it is set to zero all remaining bits of the usize value are zero.
    fn write_usize_varenc(&mut self, mut u: usize) -> io::Result<()> {
        loop {
            let mut v: u8 = (u & 0x7f) as u8;
            u >>= 7;
            if u == 0 {
                // all remaining bits of u are zero
                self.buf.push(v);
                break;
            }
            // there are more bits in u, set the 8th bit to indicate continuation
            v |= 0x80;
            self.buf.push(v);
        }
        Ok(())
    }

    /// Writes a numbered string to the buffer. Writes the unique number of the string,
    /// and writes the actual string only if was never written before.
    fn write_numbered_string_ref(&mut self, numbered_string: &NumberedString) -> io::Result<()> {
        let num = numbered_string.number;
        self.write_usize_varenc(num)?;
        if self.is_first_write_string(num) {
            // first occurrence
            numbered_string.string.map(|raw_str| {
                for c in raw_str.chars() {
                    if c.is_ascii() {
                        if c == '0' || c == '\\' {
                            self.buf.push(b'\\');
                        }
                        self.buf.push(c as u8);
                    } else {
                        let mut buf = [0; 4];
                        c.encode_utf8(&mut buf);
                        for u in buf {
                            if u == 0 {
                                break;
                            }
                            self.buf.push(u);
                        }
                    }
                }
                // write terminator
                self.buf.push(0u8);
            });
        }
        self.flush()?;
        Ok(())
    }

    /// Writes a numbered irep to the buffer. Writes the unique number of the irep,
    /// and writes the actual irep contents only if was never written before.
    fn write_numbered_irep_ref(&mut self, irep: &NumberedIrep) -> io::Result<()> {
        let num = irep.number;
        self.write_usize_varenc(num)?;

        if self.is_first_write_irep(num) {
            let id = &self.numbering.id(&irep);
            self.write_numbered_string_ref(id)?;

            for sub_idx in 0..(self.numbering.nof_sub(&irep)) {
                self.write_u8(b'S')?;
                self.write_numbered_irep_ref(&self.numbering.sub(&irep, sub_idx))?;
            }

            for named_sub_idx in 0..(self.numbering.nof_named_sub(&irep)) {
                self.write_u8(b'N')?;
                let (k, v) = self.numbering.named_sub(&irep, named_sub_idx);
                self.write_numbered_string_ref(&k)?;
                self.write_numbered_irep_ref(&v)?;
            }

            self.write_u8(0)?; // terminator
        }
        self.flush()?;
        Ok(())
    }

    /// Translates the string to its numbered version and serializes it.
    fn write_string_ref(&mut self, str: &InternedString) -> io::Result<()> {
        let numbered_string = &self.numbering.number_string(str);
        self.write_numbered_string_ref(numbered_string)
    }

    /// Translates the irep to its numbered version and serializes it.
    fn write_irep_ref(&mut self, irep: &Irep) -> io::Result<()> {
        let numbered_irep = self.numbering.number_irep(irep);
        self.write_numbered_irep_ref(&numbered_irep)
    }

    /// Writes a symbol to the byte stream.
    fn write_symbol(&mut self, symbol: &Symbol) -> io::Result<()> {
        self.write_irep_ref(&symbol.typ)?;
        self.write_irep_ref(&symbol.value)?;
        self.write_irep_ref(&symbol.location)?;
        self.write_string_ref(&symbol.name)?;
        self.write_string_ref(&symbol.module)?;
        self.write_string_ref(&symbol.base_name)?;
        self.write_string_ref(&symbol.mode)?;
        self.write_string_ref(&symbol.pretty_name)?;
        self.write_u8(0)?;

        let mut flags: usize = 0;
        flags = (flags << 1) | (symbol.is_weak) as usize;
        flags = (flags << 1) | (symbol.is_type) as usize;
        flags = (flags << 1) | (symbol.is_property) as usize;
        flags = (flags << 1) | (symbol.is_macro) as usize;
        flags = (flags << 1) | (symbol.is_exported) as usize;
        flags = (flags << 1) | (symbol.is_input) as usize;
        flags = (flags << 1) | (symbol.is_output) as usize;
        flags = (flags << 1) | (symbol.is_state_var) as usize;
        flags = (flags << 1) | (symbol.is_parameter) as usize;
        flags = (flags << 1) | (symbol.is_auxiliary) as usize;
        flags = (flags << 1) | (false) as usize; // sym.binding;
        flags = (flags << 1) | (symbol.is_lvalue) as usize;
        flags = (flags << 1) | (symbol.is_static_lifetime) as usize;
        flags = (flags << 1) | (symbol.is_thread_local) as usize;
        flags = (flags << 1) | (symbol.is_file_local) as usize;
        flags = (flags << 1) | (symbol.is_extern) as usize;
        flags = (flags << 1) | (symbol.is_volatile) as usize;

        self.write_usize_varenc(flags)?;
        self.flush()?;
        Ok(())
    }

    /// Writes a symbol table to the byte stream.
    fn write_symbol_table(&mut self, symbol_table: &SymbolTable) -> io::Result<()> {
        // Write symbol table size
        self.write_usize_varenc(symbol_table.symbol_table.len())?;

        // Write symbols
        for symbol in symbol_table.symbol_table.values() {
            self.write_symbol(symbol)?;
        }

        self.flush()?;
        Ok(())
    }

    /// Writes an empty function map to the byte stream.
    fn write_function_map(&mut self) -> io::Result<()> {
        // Write empty GOTO functions map
        self.write_usize_varenc(0)?;
        self.flush()?;
        Ok(())
    }

    /// Writes a GOTO binary file header to the byte stream.
    fn write_header(&mut self) -> io::Result<()> {
        // Write header
        self.write_u8(0x7f)?;
        self.write_u8(b'G')?;
        self.write_u8(b'B')?;
        self.write_u8(b'F')?;

        // Write goto binary version
        self.write_usize_varenc(5)?;
        self.flush()?;
        Ok(())
    }

    /// Writes the symbol table using the GOTO binary file format to the byte stream.
    fn write_file(&mut self, symbol_table: &SymbolTable) -> io::Result<()> {
        self.write_header()?;
        self.write_symbol_table(symbol_table)?;
        self.write_function_map()?;
        self.flush()?;
        Ok(())
    }
}

/// GOTO binary deserializer. Reads GOTO constructs from the byte stream of a reader.
struct GotoBinaryDeserializer<R>
where
    R: Read,
{
    /// Stream of bytes from which GOTO objects are read.
    bytes: Bytes<R>,

    /// Numbering for ireps
    numbering: IrepNumbering,

    /// Counts how many times a given irep was read.
    irep_count: Vec<usize>,

    /// Maps the irep number used in the binary stream to the new one generated by our own numbering.
    irep_map: Vec<Option<usize>>,

    /// Counts how many times a given string was read.
    string_count: Vec<usize>,

    /// Maps the string number used in the binary stream to the new one generated by our own numbering.
    string_map: Vec<Option<usize>>,
}

impl<R> GotoBinaryDeserializer<R>
where
    R: Read,
{
    /// Constructor. The reader is moved into this object and cannot be used
    /// afterwards.
    fn new(reader: R) -> Self {
        GotoBinaryDeserializer {
            bytes: reader.bytes(),
            numbering: IrepNumbering::new(),
            string_count: Vec::new(),
            string_map: Vec::new(),
            irep_count: Vec::new(),
            irep_map: Vec::new(),
        }
    }

    #[cfg(test)]
    /// Returns memory consumption and sharing statistics about the deserializer.
    fn get_stats(&self) -> GotoBinarySharingStats {
        GotoBinarySharingStats::from_deserializer(self)
    }

    /// Returns Err if the found value is not the expected value.
    fn expect<T: Eq + std::fmt::Display>(found: T, expected: T) -> io::Result<T> {
        if found != expected {
            return Err(Error::new(
                ErrorKind::Other,
                format!("expected {expected} in byte stream, found {found} instead)"),
            ));
        }
        Ok(found)
    }

    /// Adds an InternedString unique number to the "read" cache, returns true iff was never read before.
    fn is_first_read_string(&mut self, u: usize) -> bool {
        if u >= self.string_count.len() {
            self.string_count.resize(u + 1, 0);
        }
        let count = self.string_count[u];
        self.string_count[u] = count.checked_add(1).unwrap();
        count == 0
    }

    /// Maps a string number used in the byte stream to the number generated by our own numbering for that string.
    fn add_string_mapping(&mut self, num_binary: usize, num: usize) {
        if num_binary >= self.string_map.len() {
            self.string_map.resize(num_binary + 1, None);
        }
        let old = self.string_map[num_binary];
        if old.is_some() {
            panic!("string number already mapped");
        }
        self.string_map[num_binary] = Some(num);
    }

    /// Adds an Irep unique number to the "read" cache, returns true iff it was never read before.
    fn is_first_read_irep(&mut self, u: usize) -> bool {
        if u >= self.irep_count.len() {
            self.irep_count.resize(u + 1, 0);
        }
        let count = self.irep_count[u];
        self.irep_count[u] = count.checked_add(1).unwrap();
        count == 0
    }

    /// Maps an Irep number used in the byte stream to the number generated by our own numbering for that Irep.
    fn add_irep_mapping(&mut self, num_binary: usize, num: usize) {
        if num_binary >= self.irep_map.len() {
            self.irep_map.resize(num_binary + 1, None);
        }
        let old = self.irep_map[num_binary];
        if old.is_some() {
            panic!("irep number already mapped");
        }
        self.irep_map[num_binary] = Some(num);
    }

    /// Reads a u8 from the byte stream.
    fn read_u8(&mut self) -> io::Result<u8> {
        match self.bytes.next() {
            Some(Ok(u)) => Ok(u),
            Some(Err(error)) => Err(error),
            None => Err(Error::new(ErrorKind::Other, "unexpected end of input")),
        }
    }

    /// Reads a usize from the byte stream assuming 7-bit variable length encoding.
    fn read_usize_varenc(&mut self) -> io::Result<usize> {
        let mut result: usize = 0;
        let mut shift: usize = 0;
        let max_shift: usize = std::mem::size_of::<usize>() * std::mem::size_of::<u8>() * 8;
        loop {
            match self.bytes.next() {
                Some(Ok(u)) => {
                    if shift >= max_shift {
                        return Err(Error::new(
                            ErrorKind::Other,
                            "serialized value is too large to fit in usize",
                        ));
                    };
                    result |= ((u & 0x7f) as usize) << shift;
                    shift = shift.checked_add(7).unwrap();
                    if u & (0x80_u8) == 0 {
                        return Ok(result);
                    }
                }
                Some(Err(error)) => {
                    return Err(error);
                }
                None => {
                    return Err(Error::new(ErrorKind::Other, "unexpected end of input"));
                }
            }
        }
    }

    /// Reads a reference encoded string from the byte stream.
    fn read_numbered_string_ref(&mut self) -> io::Result<NumberedString> {
        let string_number_result = self.read_usize_varenc();
        let string_number = match string_number_result {
            Ok(number) => number,
            Err(error) => return Err(error),
        };
        if self.is_first_read_string(string_number) {
            // read raw string
            let mut string_buf: Vec<u8> = Vec::new();
            loop {
                match self.bytes.next() {
                    Some(Ok(u)) => {
                        match u {
                            0 => {
                                // Reached end of string
                                match String::from_utf8(string_buf) {
                                    Ok(str) => {
                                        let numbered = self.numbering.number_string(&str.intern());
                                        self.add_string_mapping(string_number, numbered.number);
                                        return Ok(numbered);
                                    }
                                    Err(error) => {
                                        return Err(Error::new(
                                            ErrorKind::Other,
                                            error.to_string(),
                                        ));
                                    }
                                }
                            }
                            b'\\' => {
                                // Found escape symbol, read the next char
                                match self.bytes.next() {
                                    Some(Ok(c)) => {
                                        string_buf.push(c);
                                    }
                                    Some(Err(error)) => {
                                        return Err(error);
                                    }
                                    None => {
                                        return Err(Error::new(
                                            ErrorKind::Other,
                                            "unexpected end of input",
                                        ));
                                    }
                                }
                            }
                            c => {
                                // Found normal char, push to buffer
                                string_buf.push(c);
                            }
                        }
                    }
                    Some(Err(error)) => {
                        // Could not read from byte stream, propagate
                        return Err(error);
                    }
                    None => {
                        // No more bytes left
                        return Err(Error::new(ErrorKind::Other, "unexpected end of input"));
                    }
                }
            }
        } else {
            // We already read this irep, fetch it from the numbering
            Ok(self
                .numbering
                .numbered_string_from_number(self.string_map[string_number].unwrap())
                .unwrap())
        }
    }

    /// Reads a NumberedIrep from the byte stream.
    fn read_numbered_irep_ref(&mut self) -> io::Result<NumberedIrep> {
        let irep_number_result = self.read_usize_varenc();
        let irep_number = match irep_number_result {
            Ok(number) => number,
            Err(error) => return Err(error),
        };

        if self.is_first_read_irep(irep_number) {
            let id = self.read_numbered_string_ref()?.number;
            let mut sub_done = false;
            let mut sub: Vec<usize> = Vec::new();
            let mut named_sub: Vec<(usize, usize)> = Vec::new();
            loop {
                // read subs and named subs one by one until the 0 terminator is found
                let c = self.read_u8()?;
                match c {
                    b'S' => {
                        if sub_done {
                            return Err(Error::new(ErrorKind::Other, "incorrect binary structure"));
                        }
                        let decoded_sub = self.read_numbered_irep_ref()?;
                        sub.push(decoded_sub.number);
                    }
                    b'N' => {
                        sub_done = true;
                        let decoded_name = self.read_numbered_string_ref()?;
                        let decoded_sub = self.read_numbered_irep_ref()?;
                        named_sub.push((decoded_name.number, decoded_sub.number));
                    }
                    0 => {
                        // Reached the end of this irep
                        // Build the key
                        let key = IrepKey::new(id, &sub, &named_sub);

                        // Insert key in the numbering
                        let numbered = self.numbering.get_or_insert(&key);

                        // Map number from the binary to new number
                        self.add_irep_mapping(irep_number, numbered.number);
                        return Ok(numbered);
                    }
                    other => {
                        return Err(Error::new(
                            ErrorKind::Other,
                            format!("unexpected character in input stream {}", other as char),
                        ));
                    }
                }
            }
        } else {
            Ok(self
                .numbering
                .numbered_irep_from_number(self.irep_map[irep_number].unwrap())
                .unwrap())
        }
    }

    /// Reads a Symbol from the byte stream.
    fn read_symbol(&mut self) -> io::Result<()> {
        // Read Irep attributes of the symbol
        let _typ = self.read_numbered_irep_ref()?;
        let _value = self.read_numbered_irep_ref()?;
        let _location = self.read_numbered_irep_ref()?;

        // Read string attributes of the symbol
        let _name = self.read_numbered_string_ref()?;
        let _module = self.read_numbered_string_ref()?;
        let _base_name = self.read_numbered_string_ref()?;
        let _mode = self.read_numbered_string_ref()?;
        let _pretty_name = self.read_numbered_string_ref()?;

        // obsolete: symordering
        let symordering = self.read_u8()?;
        Self::expect(symordering, 0)?;

        // Decode the bit-packed flags and extract bits one by one
        let flags: usize = self.read_usize_varenc()?;

        let _is_weak = (flags & (1 << 16)) != 0;
        let _is_type = (flags & (1 << 15)) != 0;
        let _is_property = (flags & (1 << 14)) != 0;
        let _is_macro = (flags & (1 << 13)) != 0;
        let _is_exported = (flags & (1 << 12)) != 0;
        let _is_input = (flags & (1 << 11)) != 0;
        let _is_output = (flags & (1 << 10)) != 0;
        let _is_state_var = (flags & (1 << 9)) != 0;
        let _is_parameter = (flags & (1 << 8)) != 0;
        let _is_auxiliary = (flags & (1 << 7)) != 0;
        let _is_binding = (flags & (1 << 6)) != 0; // deprecated
        let _is_lvalue = (flags & (1 << 5)) != 0;
        let _is_static_lifetime = (flags & (1 << 4)) != 0;
        let _is_thread_local = (flags & (1 << 3)) != 0;
        let _is_file_local = (flags & (1 << 2)) != 0;
        let _is_extern = (flags & (1 << 1)) != 0;
        let _is_volatile = (flags & 1) != 0;
        let _is_volatile = (flags & 0x1) != 0;

        let shifted_flags = flags >> 16;

        if shifted_flags != 0 {
            return Err(Error::new(
                ErrorKind::Other,
                "incorrect binary format: set bits remain in decoded symbol flags",
            ));
        }
        Ok(())
    }

    /// Reads a whole SymbolTable from the byte stream.
    fn read_symbol_table(&mut self) -> io::Result<()> {
        // Write symbol table size
        let symbol_table_len = self.read_usize_varenc()?;

        // Write symbols
        for _ in 0..symbol_table_len {
            self.read_symbol()?;
        }

        Ok(())
    }

    /// Reads an empty function map from the byte stream.
    fn read_function_map(&mut self) -> io::Result<()> {
        let goto_function_len = self.read_usize_varenc()?;
        Self::expect(goto_function_len, 0)?;
        Ok(())
    }

    /// Reads a GOTO binary header from the byte stream.
    fn read_header(&mut self) -> io::Result<()> {
        // Read header
        Self::expect(self.read_u8().unwrap(), 0x7f)?;
        Self::expect(self.read_u8().unwrap(), b'G')?;
        Self::expect(self.read_u8().unwrap(), b'B')?;
        Self::expect(self.read_u8().unwrap(), b'F')?;

        // Read goto binary version
        let goto_binary_version = self.read_usize_varenc()?;
        if goto_binary_version != 5 {
            return Err(Error::new(
                ErrorKind::Other,
                format!(
                    "unsupported GOTO binary version: {}. Supported version: {}",
                    goto_binary_version, 5
                ),
            ));
        }
        Ok(())
    }

    /// Read a GOTO binary file from the byte stream.
    fn read_file(&mut self) -> io::Result<()> {
        self.read_header()?;
        self.read_symbol_table()?;
        self.read_function_map()?;
        Ok(())
    }
}

////////////////////////////////////////
//// Dynamic memory usage computation
////////////////////////////////////////
#[cfg(test)]
use memuse::DynamicUsage;

#[cfg(test)]
impl DynamicUsage for NumberedIrep {
    fn dynamic_usage(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn dynamic_usage_bounds(&self) -> (usize, Option<usize>) {
        let s = std::mem::size_of::<IrepKey>();
        (s, Some(s))
    }
}

#[cfg(test)]
impl DynamicUsage for IrepKey {
    fn dynamic_usage(&self) -> usize {
        std::mem::size_of::<Self>() + self.numbers.dynamic_usage()
    }

    fn dynamic_usage_bounds(&self) -> (usize, Option<usize>) {
        let (lower, upper) = self.numbers.dynamic_usage_bounds();
        let s = std::mem::size_of::<Self>();
        (lower + s, upper.map(|x| x + s))
    }
}

#[cfg(test)]
impl DynamicUsage for IrepNumberingInv {
    fn dynamic_usage(&self) -> usize {
        std::mem::size_of::<Self>() + self.index.dynamic_usage() + self.keys.dynamic_usage()
    }

    fn dynamic_usage_bounds(&self) -> (usize, Option<usize>) {
        let (lindex, uindex) = self.index.dynamic_usage_bounds();
        let (lkeys, ukeys) = self.keys.dynamic_usage_bounds();
        let s = std::mem::size_of::<IrepKey>();
        (lindex + lkeys + s, uindex.and_then(|s1| ukeys.map(|s2| s1 + s2 + s)))
    }
}

#[cfg(test)]
impl DynamicUsage for InternedString {
    fn dynamic_usage(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn dynamic_usage_bounds(&self) -> (usize, Option<usize>) {
        let s = std::mem::size_of::<Self>();
        (s, Some(s))
    }
}

#[cfg(test)]
impl DynamicUsage for NumberedString {
    fn dynamic_usage(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn dynamic_usage_bounds(&self) -> (usize, Option<usize>) {
        let s = std::mem::size_of::<Self>();
        (s, Some(s))
    }
}

#[cfg(test)]
impl DynamicUsage for IrepNumbering {
    fn dynamic_usage(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.string_cache.dynamic_usage()
            + self.inv_string_cache.dynamic_usage()
            + self.cache.dynamic_usage()
            + self.inv_cache.dynamic_usage()
    }

    fn dynamic_usage_bounds(&self) -> (usize, Option<usize>) {
        let s = std::mem::size_of::<Self>();
        let (l1, u1) = self.string_cache.dynamic_usage_bounds();
        let (l2, u2) = self.inv_string_cache.dynamic_usage_bounds();
        let (l3, u3) = self.cache.dynamic_usage_bounds();
        let (l4, u4) = self.inv_cache.dynamic_usage_bounds();
        let l = l1 + l2 + l3 + l4 + s;
        let u = u1
            .and_then(|u1| u2.and_then(|u2| u3.and_then(|u3| u4.map(|u4| u1 + u2 + u3 + u4 + s))));
        (l, u)
    }
}

#[cfg(test)]
impl<'a, W> DynamicUsage for GotoBinarySerializer<'a, W>
where
    W: Write,
{
    fn dynamic_usage(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.buf.dynamic_usage()
            + self.numbering.dynamic_usage()
            + self.irep_count.dynamic_usage()
            + self.string_count.dynamic_usage()
    }

    fn dynamic_usage_bounds(&self) -> (usize, Option<usize>) {
        let s = std::mem::size_of::<Self>();
        let (l1, u1) = self.buf.dynamic_usage_bounds();
        let (l2, u2) = self.numbering.dynamic_usage_bounds();
        let (l3, u3) = self.irep_count.dynamic_usage_bounds();
        let (l4, u4) = self.string_count.dynamic_usage_bounds();
        let l = l1 + l2 + l3 + l4 + s;
        let u = u1
            .and_then(|u1| u2.and_then(|u2| u3.and_then(|u3| u4.map(|u4| u1 + u2 + u3 + u4 + s))));
        (l, u)
    }
}

#[cfg(test)]
impl<R> DynamicUsage for GotoBinaryDeserializer<R>
where
    R: Read,
{
    fn dynamic_usage(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.numbering.dynamic_usage()
            + self.irep_count.dynamic_usage()
            + self.irep_map.dynamic_usage()
            + self.string_count.dynamic_usage()
            + self.string_map.dynamic_usage()
    }

    fn dynamic_usage_bounds(&self) -> (usize, Option<usize>) {
        let s = std::mem::size_of::<Self>();
        let (l1, u1) = self.numbering.dynamic_usage_bounds();
        let (l2, u2) = self.irep_count.dynamic_usage_bounds();
        let (l3, u3) = self.irep_map.dynamic_usage_bounds();
        let (l4, u4) = self.string_count.dynamic_usage_bounds();
        let (l5, u5) = self.string_map.dynamic_usage_bounds();
        let l = l1 + l2 + l3 + l4 + l5 + s;
        let u = u1.and_then(|u1| {
            u2.and_then(|u2| {
                u3.and_then(|u3| u4.and_then(|u4| u5.map(|u5| u1 + u2 + u3 + u4 + u5 + s)))
            })
        });
        (l, u)
    }
}

#[cfg(test)]
#[derive(Debug)]
/// Structural sharing statistics
struct SharingStats {
    // Number of structurally unique objects
    _nof_unique: usize,

    // Minimum count for a unique object
    _min_count: usize,

    // Unique identifier of the min count object
    _min_id: Option<usize>,

    // Maximum count for a unique object
    _max_count: usize,

    // Unique identifier of the max count object
    _max_id: Option<usize>,

    // Average count for objects
    _avg_count: f64,
}

#[cfg(test)]
impl SharingStats {
    fn new(elems: &[usize]) -> Self {
        let mut nof_unique: usize = 0;
        let mut min_count: usize = usize::MAX;
        let mut min_id: Option<usize> = None;
        let mut max_count: usize = 0;
        let mut max_id: Option<usize> = None;
        let mut avg_count: f64 = 0.0;

        for (id, count) in elems.iter().enumerate() {
            if *count == 0 {
                continue;
            }
            if *count < min_count {
                min_count = *count;
                min_id = Some(id);
            };
            if *count > max_count {
                max_count = *count;
                max_id = Some(id);
            };
            nof_unique = nof_unique + 1;
            let incr = (*count as f64 - avg_count) / (nof_unique as f64);
            avg_count = avg_count + incr;
        }
        SharingStats {
            _nof_unique: nof_unique,
            _min_count: min_count,
            _min_id: min_id,
            _max_count: max_count,
            _max_id: max_id,
            _avg_count: avg_count,
        }
    }
}

#[cfg(test)]
#[derive(Debug)]
/// Statistics for GotoBinarySerializer.
struct GotoBinarySharingStats {
    /// Number of bytes used by the serializer
    _allocated_bytes: usize,

    /// Sharing statistics for NumberedStrings
    _string_stats: SharingStats,

    /// Sharing statistics for NumberedIreps
    _irep_stats: SharingStats,
}

#[cfg(test)]
impl GotoBinarySharingStats {
    fn from_serializer<'a, W: Write>(s: &GotoBinarySerializer<'a, W>) -> Self {
        GotoBinarySharingStats {
            _allocated_bytes: s.dynamic_usage(),
            _string_stats: SharingStats::new(&s.string_count),
            _irep_stats: SharingStats::new(&s.irep_count),
        }
    }

    fn from_deserializer<R: Read>(s: &GotoBinaryDeserializer<R>) -> Self {
        GotoBinarySharingStats {
            _allocated_bytes: s.dynamic_usage(),
            _string_stats: SharingStats::new(&s.string_count),
            _irep_stats: SharingStats::new(&s.irep_count),
        }
    }
}

/// Unit tests for GOTO binary serialization/deserialization.
#[cfg(test)]
mod tests {
    use super::GotoBinarySerializer;
    use super::IrepNumbering;
    use crate::cbmc_string::InternString;
    use crate::irep::goto_binary_serde::GotoBinaryDeserializer;
    use crate::irep::Irep;
    use crate::irep::IrepId;
    use crate::linear_map;
    use crate::InternedString;
    use linear_map::LinearMap;

    /// Utility function : creates a Irep representing a single symbol.
    fn make_symbol_expr(identifier: &str) -> Irep {
        Irep {
            id: IrepId::Symbol,
            sub: vec![],
            named_sub: linear_map![(IrepId::Identifier, Irep::just_string_id(identifier),)],
        }
    }

    /// Utility function: creates an expression by folding the symbol expressions with the given operator.
    fn fold_with_op(identifiers: &Vec<&str>, id: IrepId) -> Irep {
        identifiers.iter().fold(make_symbol_expr("dummy"), |acc, identifier| Irep {
            id: id.clone(),
            sub: vec![acc, make_symbol_expr(identifier)],
            named_sub: LinearMap::new(),
        })
    }

    #[test]
    /// Create two structurally identical ireps and check that they get the same number.
    fn test_irep_numbering_eq() {
        let mut numbering = IrepNumbering::new();
        let identifiers = vec![
            "foo", "bar", "baz", "zab", "rab", "oof", "foo", "bar", "baz", "zab", "rab", "oof",
        ];
        let num1 = numbering.number_irep(&fold_with_op(&identifiers, IrepId::And));
        let num2 = numbering.number_irep(&fold_with_op(&identifiers, IrepId::And));
        assert_eq!(num1, num2);
    }

    #[test]
    /// Create two ireps with different named subs and check that they get different numbers.
    fn test_irep_numbering_ne_named_sub() {
        let mut numbering = IrepNumbering::new();

        let identifiers1 = vec![
            "foo", "bar", "baz", "zab", "rab", "oof", "foo", "bar", "baz", "zab", "rab", "oof",
        ];
        let num1 = numbering.number_irep(&fold_with_op(&identifiers1, IrepId::And));

        let identifiers2 = vec![
            "foo", "bar", "HERE", "zab", "rab", "oof", "foo", "bar", "baz", "zab", "rab", "oof",
        ];
        let num2 = numbering.number_irep(&fold_with_op(&identifiers2, IrepId::And));
        assert_ne!(num1, num2);
    }

    #[test]
    /// Create two ireps with different ids and check that they get different numbers.
    fn test_irep_numbering_ne_id() {
        let mut numbering = IrepNumbering::new();

        let identifiers = vec![
            "foo", "bar", "baz", "zab", "rab", "oof", "foo", "bar", "baz", "zab", "rab", "oof",
        ];
        let num1 = numbering.number_irep(&fold_with_op(&identifiers, IrepId::And));
        let num2 = numbering.number_irep(&fold_with_op(&identifiers, IrepId::Or));

        assert_ne!(num1, num2);
    }

    #[test]
    /// Write and read back all possible u8 values.
    fn test_write_u8() {
        let mut vec: Vec<u8> = Vec::new();
        let mut serializer = GotoBinarySerializer::new(&mut vec);

        // write all possible u8 values
        for u in std::u8::MIN..std::u8::MAX {
            serializer.write_u8(u).unwrap();
        }
        serializer.flush().unwrap();

        // read back from byte stream
        for u in std::u8::MIN..std::u8::MAX {
            assert_eq!(vec[u as usize], u);
        }
    }

    #[test]
    /// Write and read back usize values covering the whole usize bit-width.
    fn test_write_usize() {
        // Generate all powers of two to cover the whole bitwidth
        let mut powers_of_two: Vec<usize> = Vec::new();
        powers_of_two.push(0);
        for i in 0..usize::BITS {
            let num = 1usize << i;
            powers_of_two.push(num);
        }
        powers_of_two.push(usize::MAX);

        // Serialize using variable length encoding
        let mut vec: Vec<u8> = Vec::new();
        let mut serializer = GotoBinarySerializer::new(&mut vec);
        for number in powers_of_two.iter() {
            serializer.write_usize_varenc(*number).unwrap();
        }
        serializer.flush().unwrap();

        // Deserialize byte stream and check equality
        let mut deserializer = GotoBinaryDeserializer::new(std::io::Cursor::new(vec));
        for number in powers_of_two.iter() {
            let decoded = deserializer.read_usize_varenc().unwrap();
            assert_eq!(decoded, *number);
        }
    }

    #[test]
    /// Write and read back unique strings.
    fn test_write_read_unique_string_ref() {
        let strings: Vec<InternedString> = vec![
            "some_string".intern(),
            "some other string".intern(),
            "some string containing 0 and some other things".intern(),
            "some string containing \\ and some other things".intern(),
            "some string containing \\ and # and $ and % and \n \t and 1231231".intern(),
        ];

        // Serialize unique strings
        let mut vec: Vec<u8> = Vec::new();
        let mut serializer = GotoBinarySerializer::new(&mut vec);
        for string in strings.iter() {
            serializer.write_string_ref(string).unwrap();
        }
        serializer.flush().unwrap();

        // Deserialize contents one by one and check equality
        let mut deserializer = GotoBinaryDeserializer::new(std::io::Cursor::new(vec));
        for string in strings.iter() {
            let decoded = deserializer.read_numbered_string_ref().unwrap().string;
            assert_eq!(decoded, *string);
        }
    }

    #[test]
    /// Write and read back repeated strings.
    fn test_write_read_multiple_string_ref() {
        let mut vec: Vec<u8> = Vec::new();
        let foo = String::from("foo").intern();
        let bar = String::from("bar").intern();
        let baz = String::from("baz").intern();
        let strings = vec![foo, bar, foo, bar, foo, baz, baz, bar, foo];

        // Serialize the same strings several times in arbitrary order
        let mut serializer = GotoBinarySerializer::new(&mut vec);
        for string in strings.iter() {
            serializer.write_string_ref(&string).unwrap();
        }
        println!("Serializer stats {:?}", serializer.get_stats());

        // Deserialize the byte stream and check equality
        let mut deserializer = GotoBinaryDeserializer::new(std::io::Cursor::new(vec));
        for string in strings.iter() {
            let decoded = deserializer.read_numbered_string_ref().unwrap().string;
            assert_eq!(decoded.to_string(), string.to_string());
        }
        println!("Deserializer stats {:?}", deserializer.get_stats());
    }

    #[test]
    /// Write and read back distinct ireps.
    fn test_write_irep_ref() {
        let identifiers1 = vec!["foo", "bar", "baz", "same", "zab", "rab", "oof"];
        let irep1 = &fold_with_op(&identifiers1, IrepId::And);

        let mut vec: Vec<u8> = Vec::new();
        let mut serializer = GotoBinarySerializer::new(&mut vec);

        // Number an irep
        let num1 = serializer.numbering.number_irep(&irep1);

        // Number an structurally different irep
        let identifiers2 = vec!["foo", "bar", "baz", "different", "zab", "rab", "oof"];
        let irep2 = &fold_with_op(&identifiers2, IrepId::And);
        let num2 = serializer.numbering.number_irep(&irep2);

        // Check that they have the different numbers.
        assert_ne!(num1, num2);

        // write both numbered ireps
        serializer.write_numbered_irep_ref(&num1).unwrap();
        serializer.write_numbered_irep_ref(&num2).unwrap();

        // check that the serializer knows it wrote the same irep twice
        assert!(serializer.irep_count[num1.number] == 1);
        assert!(serializer.irep_count[num2.number] == 1);
        println!("Serializer stats {:?}", serializer.get_stats());

        // Deserialize two ireps from the byte stream
        let mut deserializer = GotoBinaryDeserializer::new(std::io::Cursor::new(vec));
        let num3 = deserializer.read_numbered_irep_ref().unwrap();
        let num4 = deserializer.read_numbered_irep_ref().unwrap();
        println!("Deserializer stats {:?}", deserializer.get_stats());

        // Check that they have different numbers.
        assert_ne!(num3, num4);
    }

    #[test]
    /// Write and read back several identical ireps.
    fn test_write_read_irep_ref() {
        let identifiers = vec![
            "foo", "bar", "baz", "zab", "rab", "oof", "foo", "bar", "baz", "zab", "rab", "oof",
        ];

        let mut vec: Vec<u8> = Vec::new();
        {
            // Write two structurally identical ireps
            let mut serializer = GotoBinarySerializer::new(&mut vec);
            let irep1 = &fold_with_op(&identifiers, IrepId::And);
            let irep2 = &fold_with_op(&identifiers, IrepId::And);
            serializer.write_irep_ref(irep1).unwrap();
            serializer.write_irep_ref(irep2).unwrap();
            serializer.write_irep_ref(irep1).unwrap();
            serializer.write_irep_ref(irep2).unwrap();
            serializer.write_irep_ref(irep1).unwrap();
            serializer.write_irep_ref(irep1).unwrap();
            println!("Serializer stats {:?}", serializer.get_stats());
        }

        {
            // Deserialize the byte stream and check that we get the same numbered ireps
            let mut deserializer = GotoBinaryDeserializer::new(std::io::Cursor::new(vec));
            let irep1 = deserializer.read_numbered_irep_ref().unwrap();
            let irep2 = deserializer.read_numbered_irep_ref().unwrap();
            let irep3 = deserializer.read_numbered_irep_ref().unwrap();
            let irep4 = deserializer.read_numbered_irep_ref().unwrap();
            let irep5 = deserializer.read_numbered_irep_ref().unwrap();
            let irep6 = deserializer.read_numbered_irep_ref().unwrap();
            println!("Deserializer stats {:?}", deserializer.get_stats());
            assert_eq!(irep1, irep2);
            assert_eq!(irep1, irep3);
            assert_eq!(irep1, irep4);
            assert_eq!(irep1, irep5);
            assert_eq!(irep1, irep6);
        }
    }
}
