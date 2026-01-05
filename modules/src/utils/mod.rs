pub mod any_of;
pub mod bytes;
pub mod class;
pub mod compression;
pub mod console;
pub mod ctx;
pub mod encoding;
pub mod error;
pub mod fs;
pub mod hash;
pub mod io;
pub mod json;
pub mod mc_oneshot;
pub mod module;
pub mod numbers;
pub mod object;
pub mod options;
pub mod primordials;
pub mod provider;
pub mod result;
pub mod test;
pub mod time;

#[macro_export]
macro_rules! count_members {
    () => (0);
    ($head:tt $(,$tail:tt)*) => (1 + count_members!($($tail),*));
}

#[macro_export]
macro_rules! iterable_enum {
    ($name:ident, $($variant:ident),*) => {
        impl $name {
            const VARIANTS: &'static [$name] = &[$($name::$variant,)*];
            pub fn iter() -> std::slice::Iter<'static, $name> {
                Self::VARIANTS.iter()
            }

            #[allow(dead_code)]
            fn _ensure_all_variants(s: Self) {
                match s {
                    $($name::$variant => {},)*
                }
            }
        }
    };
}

#[macro_export]
macro_rules! str_enum {
    ($name:ident, $($variant:ident => $str:expr),*) => {
        impl $name {
            pub fn as_str(&self) -> &'static str {
                match self {
                    $($name::$variant => $str,)*
                }
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl TryFrom<&str> for $name {
            type Error = String;
            fn try_from(s: &str) -> std::result::Result<Self, Self::Error> {
                match s {
                    $($str => Ok($name::$variant),)*
                    _ => Err(["'", s, "' not available"].concat())
                }
            }
        }
    };
}
