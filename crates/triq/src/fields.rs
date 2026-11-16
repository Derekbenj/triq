//! Which numbers a message holds, and where: what a tool needs to show the
//! inside of a message whose type it was not built with.
//!
//! iceoryx2 knows a message type by its name, size and alignment. A type that
//! derives [`Fields`] also lists its numbers, and the runtime that owns an
//! output passes the list on to whoever asks. Nothing of it travels with the
//! messages.
use serde::{Deserialize, Serialize};

/// One number inside a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    /// As a person would write it: `translation[0]`, `pose.rotation[3]`.
    pub name: String,
    /// How many bytes into the message it begins.
    pub at: usize,
    pub kind: Scalar,
}

/// How a number is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scalar {
    Bool,
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

impl Field {
    /// The number as it stands in `message`, the bytes of a message as they
    /// lie in memory. `None` if they end before the number does.
    pub fn read(&self, message: &[u8]) -> Option<f64> {
        let bytes = message.get(self.at..)?;
        // The first bytes of what is left, taken as the number's own type.
        macro_rules! number {
            ($type:ty) => {
                <$type>::from_ne_bytes(*bytes.first_chunk()?) as f64
            };
        }
        Some(match self.kind {
            Scalar::Bool => f64::from(u8::from(*bytes.first()? != 0)),
            Scalar::U8 => number!(u8),
            Scalar::U16 => number!(u16),
            Scalar::U32 => number!(u32),
            Scalar::U64 => number!(u64),
            Scalar::I8 => number!(i8),
            Scalar::I16 => number!(i16),
            Scalar::I32 => number!(i32),
            Scalar::I64 => number!(i64),
            Scalar::F32 => number!(f32),
            Scalar::F64 => number!(f64),
        })
    }
}

/// The numbers inside what one output sends.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    /// The numbers of a message; for a slice, of one element of it.
    pub payload: Vec<Field>,
    /// The numbers of the port's header, by where they are in what travels
    /// beside the message: after triq's [`Stamp`](crate::Stamp).
    pub header: Vec<Field>,
}

/// A message type that lists the numbers it holds. Derive it, next to `ZeroCopySend`:
///
/// ```
/// use triq::prelude::*;
///
/// #[derive(Debug, Clone, Copy, Default, ZeroCopySend, Fields)]
/// #[repr(C)]
/// struct Pose {
///     translation: [f32; 3],
///     rotation: [f32; 4],
/// }
/// ```
///
/// Its numbers are then `translation[0]` to `rotation[3]`. A type whose
/// numbers cannot be listed, an enum for one, says so with
/// `impl Fields for T {}`, and tools show nothing of what is inside it.
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not say which numbers it holds",
    note = "add `Fields` to its derives, or write `impl triq::Fields for {Self} {{}}` if they cannot be listed"
)]
pub trait Fields {
    /// Add to `found` the numbers of a value of this type that is called
    /// `name` and begins `at` bytes into its message.
    fn fields(_name: &str, _at: usize, _found: &mut Vec<Field>) {}
}

macro_rules! numbers {
    ($($type:ty: $kind:ident),*) => {$(
        impl Fields for $type {
            fn fields(name: &str, at: usize, found: &mut Vec<Field>) {
                // A message that is nothing but a number has no name for it.
                let name = if name.is_empty() { "value" } else { name };
                found.push(Field { name: name.to_owned(), at, kind: Scalar::$kind });
            }
        }
    )*};
}

numbers!(bool: Bool, u8: U8, u16: U16, u32: U32, u64: U64, i8: I8, i16: I16, i32: I32, i64: I64, f32: F32, f64: F64);

impl Fields for () {}

/// How many elements of an array are listed. A picture kept inside a message
/// is not a million numbers for a person to choose from.
const LISTED: usize = 64;

/// The elements of an array are called by their place in it.
impl<T: Fields, const N: usize> Fields for [T; N] {
    fn fields(name: &str, at: usize, found: &mut Vec<Field>) {
        for index in 0..N.min(LISTED) {
            T::fields(&format!("{name}[{index}]"), at + index * size_of::<T>(), found);
        }
    }
}

/// What a member of a struct is called: `rotation` in a message, `pose.rotation` inside its `pose`.
#[doc(hidden)]
pub fn member(of: &str, name: &str) -> String {
    if of.is_empty() { name.to_owned() } else { format!("{of}.{name}") }
}
