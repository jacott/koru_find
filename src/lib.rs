pub(crate) const LOCK_SHOULD_BE_OK: &str = "Lock should be ok";

pub mod pattern;
pub mod server;

#[macro_export]
macro_rules! fixme {
    ($a:expr) => {{
        extern crate std;
        std::eprintln!(
            // split so that not found when looking for the word in an editor
            "FIXME\
             ! at ./{}:{}:{}\n{:?}",
            file!(),
            line!(),
            column!(),
            $a,
        )
    }};
}
