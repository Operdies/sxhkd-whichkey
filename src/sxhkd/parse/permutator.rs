use std::marker::PhantomData;

pub struct Front {}
pub struct Back {}
trait PermutationOrder {}
impl PermutationOrder for Front {}
impl PermutationOrder for Back {}

pub struct Permutator<T> {
    bounds: Vec<usize>,
    current: Vec<usize>,
    first: bool,
    zeroes: bool,
    mode: PhantomData<T>,
}

pub struct Permute {}
#[allow(unused)]
impl Permute {
    pub fn front(set: &[usize]) -> Permutator<Front> {
        Permutator::<Front>::new(set)
    }
    pub fn front_first(set: &[usize]) -> Vec<Vec<usize>> {
        Permutator::<Front>::all(set)
    }
    pub fn back(set: &[usize]) -> Permutator<Back> {
        Permutator::<Back>::new(set)
    }
    pub fn back_first(set: &[usize]) -> Vec<Vec<usize>> {
        Permutator::<Back>::all(set)
    }
}

impl Permutator<Back> {
    fn new(set: &[usize]) -> Self {
        Permutator {
            bounds: set.to_vec(),
            current: vec![0; set.len()],
            first: true,
            zeroes: set.iter().any(|c| *c == 0),
            mode: PhantomData,
        }
    }
    fn all(set: &[usize]) -> Vec<Vec<usize>> {
        Self::new(set).collect()
    }
}

impl Permutator<Front> {
    fn new(set: &[usize]) -> Self {
        Permutator {
            bounds: set.to_vec(),
            current: vec![0; set.len()],
            first: true,
            zeroes: set.iter().any(|c| *c == 0),
            mode: PhantomData,
        }
    }
    fn all(set: &[usize]) -> Vec<Vec<usize>> {
        Self::new(set).collect()
    }
}

impl Iterator for Permutator<Back> {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.zeroes {
            return None;
        }
        if self.first {
            self.first = false;
            return Some(self.current.clone());
        }
        // [0,0] -> [0,1], [0,2] -> [1,0] -> [1,1] -> [1,2] -> None
        for i in (0..self.current.len()).rev() {
            if self.current[i] < (self.bounds[i] - 1) {
                self.current[i] += 1;
                for i in i + 1..self.current.len() {
                    self.current[i] = 0;
                }
                return Some(self.current.clone());
            }
        }
        None
    }
}

impl Iterator for Permutator<Front> {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.zeroes {
            return None;
        }
        if self.first {
            self.first = false;
            return Some(self.current.clone());
        }
        // [0,0] -> [1,0], [0,1] -> [1,1] -> [0,2] -> [1,2] -> None
        for i in 0..self.current.len() {
            if self.current[i] < (self.bounds[i] - 1) {
                self.current[i] += 1;
                for i in 0..i {
                    self.current[i] = 0;
                }
                return Some(self.current.clone());
            }
        }
        None
    }
}

mod test_permutations {
    #[allow(unused)]
    use super::Permute;
    #[test]
    fn test_back() {
        let all_back = Permute::back_first(&[2, 3]);
        assert_eq!(6, all_back.len());
        assert!(matches!(&all_back[0][..], [0, 0]));
        assert!(matches!(&all_back[1][..], [0, 1]));
        assert!(matches!(&all_back[2][..], [0, 2]));
        assert!(matches!(&all_back[3][..], [1, 0]));
        assert!(matches!(&all_back[4][..], [1, 1]));
        assert!(matches!(&all_back[5][..], [1, 2]));
    }
    #[test]
    fn test_front() {
        let all_front = Permute::front_first(&[2, 3]);
        assert_eq!(6, all_front.len());
        assert!(matches!(&all_front[0][..], [0, 0]));
        assert!(matches!(&all_front[1][..], [1, 0]));
        assert!(matches!(&all_front[2][..], [0, 1]));
        assert!(matches!(&all_front[3][..], [1, 1]));
        assert!(matches!(&all_front[4][..], [0, 2]));
        assert!(matches!(&all_front[5][..], [1, 2]));
    }
}
