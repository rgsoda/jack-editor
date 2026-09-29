/// One fruit, as it came off the tree.
#[derive(Clone, Debug)]
pub struct Fruit {
    pub name: String,
    pub grams: u32,
    pub ripe: bool,
}

impl Fruit {
    pub fn new(name: &str, grams: u32, ripe: bool) -> Self {
        Fruit { name: name.to_string(), grams, ripe }
    }

    pub fn is_worth_keeping(&self) -> bool {
        self.ripe && self.grams > 10
    }
}

/// A basket with a weight it will not go over.
pub struct Basket {
    label: String,
    limit: u32,
    held: Vec<Fruit>,
}

impl Basket {
    pub fn new(label: &str, limit: u32) -> Self {
        Basket { label: label.to_string(), limit, held: Vec::new() }
    }

    pub fn weight(&self) -> u32 {
        self.held.iter().map(|f| f.grams).sum()
    }

    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn add(&mut self, fruit: Fruit) -> bool {
        if !fruit.is_worth_keeping() {
            return false;
        }
        if self.weight() + fruit.grams > self.limit {
            return false;
        }
        self.held.push(fruit);
        true
    }
}
