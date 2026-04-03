/// Trait for converting single or multiple items into `Vec<T>`.
///
/// Enables builder methods to accept flexible arguments:
/// ```ignore
/// .from("users")              // single &str
/// .from(some_target)          // single item
/// .from(vec![t1, t2])         // vec
/// .from([t1, t2])             // array
/// .from(slice.as_slice())     // slice
/// ```
pub trait IntoVec<T> {
    fn into_vec(self) -> Vec<T>;
}

impl<T> IntoVec<T> for Vec<T> {
    fn into_vec(self) -> Vec<T> {
        self
    }
}

impl<T, const N: usize> IntoVec<T> for [T; N] {
    fn into_vec(self) -> Vec<T> {
        self.into()
    }
}

impl<T: Clone> IntoVec<T> for &[T] {
    fn into_vec(self) -> Vec<T> {
        self.to_vec()
    }
}
