use std::fmt::Debug;

use quote::ToTokens;

pub struct BetterToTokenDebug<'a, T>(pub &'a T);

impl<T> Debug for BetterToTokenDebug<'_, T>
where
    T: ToTokens,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BetterToTokenDebug")
            .field(&self.0.to_token_stream().to_string())
            .finish()
    }
}
