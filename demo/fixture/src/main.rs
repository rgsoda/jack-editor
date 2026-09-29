mod shapes;

use shapes::{Basket, Fruit};

/// Everything the orchard produced this season.
fn harvest() -> Vec<Fruit> {
    let mut picked = Vec::new();
    for (name, grams, ripe) in ROWS {
        picked.push(Fruit::new(name, grams, ripe));
    }
    picked
}

const ROWS: [(&str, u32, bool); 6] = [
    ("quince", 180, true),
    ("medlar", 95, false),
    ("damson", 22, true),
    ("greengage", 31, true),
    ("mulberry", 8, false),
    ("sloe", 4, true),
];

fn heaviest(fruit: &[Fruit]) -> Option<&Fruit> {
    fruit.iter().max_by_key(|f| f.grams)
}

fn main() {
    let picked = harvest();
    let mut basket = Basket::new("the long barrow", 2400);

    for fruit in &picked {
        if !basket.add(fruit.clone()) {
            println!("no room for {}", fruit.name);
        }
    }

    println!("{} fruit, {} grams", basket.len(), basket.weight());
    if let Some(big) = heaviest(&picked) {
        println!("heaviest: {} at {}g", big.name, big.grams);
    }
}
