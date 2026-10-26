use std::fmt::Debug;

pub trait Element: Clone + Debug + Default + Send + Sync {}

impl<T: Clone + Debug + Default + Send + Sync> Element for T {}
