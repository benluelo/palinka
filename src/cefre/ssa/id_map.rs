use std::{
    fmt::Debug,
    marker::PhantomData,
    ops::{Index, IndexMut},
};

pub struct IdMap<I: Id, T> {
    // option so that items can be removed
    map: Vec<Option<T>>,
    __: PhantomData<fn() -> I>,
}

impl<I: Id, T> Index<I> for IdMap<I, T> {
    type Output = T;

    fn index(&self, i: I) -> &Self::Output {
        self.map[i.as_usize()].as_ref().unwrap()
    }
}

impl<I: Id, T> IndexMut<I> for IdMap<I, T> {
    fn index_mut(&mut self, i: I) -> &mut Self::Output {
        self.map[i.as_usize()].as_mut().unwrap()
    }
}

impl<I: Id, T> Default for IdMap<I, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: Id, T> IdMap<I, T> {
    pub fn new() -> Self {
        Self { map: vec![], __: PhantomData }
    }

    pub fn iter(&self) -> impl Iterator<Item = (I, &T)> {
        self.map.iter().enumerate().filter_map(|(k, v)| v.as_ref().map(|v| (I::from_usize(k), v)))
    }

    pub fn insert(&mut self, t: T) -> I {
        self.map.push(Some(t));
        I::from_usize(self.map.len() - 1)
    }

    pub fn get(&self, i: I) -> &T {
        self.map[i.as_usize()].as_ref().unwrap()
    }

    pub fn remove(&mut self, i: I) {
        self.map[i.as_usize()].take().unwrap();
    }

    pub fn get_mut(&mut self, i: I) -> &mut T {
        self.map[i.as_usize()].as_mut().unwrap()
    }
}

impl<I: Id + Debug, T: Debug> Debug for IdMap<I, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(
                self.map
                    .iter()
                    .enumerate()
                    .filter_map(|(i, t)| t.as_ref().map(|t| (I::from_usize(i), t))),
            )
            .finish()
    }
}

pub trait Id: Debug + Copy {
    fn as_usize(self) -> usize;
    fn from_usize(id: usize) -> Self;
}
