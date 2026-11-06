//! Чистая логика каталога: себестоимость составных товаров и защита от циклов в составе.
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// Узел каталога для расчётов.
#[derive(Debug, Clone)]
pub struct Node<K> {
    pub purchase_price: f64,
    pub extra_cost: f64,
    pub composite: bool,
    pub ingredients: Vec<(K, f64)>,
}

const MAX_DEPTH: usize = 16;

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

fn cost<K: Copy + Eq + Hash>(nodes: &HashMap<K, Node<K>>, id: K, depth: usize) -> f64 {
    if depth > MAX_DEPTH {
        return 0.0;
    }
    let Some(n) = nodes.get(&id) else { return 0.0 };
    if n.composite {
        n.ingredients.iter().map(|(i, q)| q * cost(nodes, *i, depth + 1)).sum::<f64>() + n.extra_cost
    } else {
        n.purchase_price + n.extra_cost
    }
}

/// Себестоимость 1 шт: обычный товар — закупочная + доп. расходы;
/// составной — Σ(qty × себестоимость ингредиента) + доп. расходы.
pub fn unit_cost<K: Copy + Eq + Hash>(nodes: &HashMap<K, Node<K>>, id: K) -> f64 {
    round4(cost(nodes, id, 0))
}

/// Создаст ли состав `ingredients` для товара `product` цикл (товар прямо или косвенно входит сам в себя).
/// Существующий состав самого `product` в `nodes` игнорируется — он будет заменён.
pub fn creates_cycle<K: Copy + Eq + Hash>(nodes: &HashMap<K, Node<K>>, product: K, ingredients: &[K]) -> bool {
    let mut seen: HashSet<K> = HashSet::new();
    let mut stack: Vec<K> = ingredients.to_vec();
    while let Some(k) = stack.pop() {
        if k == product {
            return true;
        }
        if !seen.insert(k) {
            continue;
        }
        if let Some(n) = nodes.get(&k) {
            stack.extend(n.ingredients.iter().map(|(i, _)| *i));
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(price: f64, extra: f64) -> Node<u32> {
        Node { purchase_price: price, extra_cost: extra, composite: false, ingredients: vec![] }
    }

    #[test]
    fn simple_and_composite_cost() {
        let mut m = HashMap::new();
        m.insert(1, leaf(100.0, 5.0)); // молоко: 105
        m.insert(2, leaf(40.0, 0.0)); // сироп: 40
        m.insert(
            3,
            Node { purchase_price: 999.0, extra_cost: 10.0, composite: true, ingredients: vec![(1, 0.2), (2, 0.5)] },
        );
        assert_eq!(unit_cost(&m, 1), 105.0);
        // 0.2*105 + 0.5*40 + 10 = 51
        assert_eq!(unit_cost(&m, 3), 51.0);
        assert_eq!(unit_cost(&m, 99), 0.0);
    }

    #[test]
    fn nested_composite() {
        let mut m = HashMap::new();
        m.insert(1, leaf(10.0, 0.0));
        m.insert(2, Node { purchase_price: 0.0, extra_cost: 0.0, composite: true, ingredients: vec![(1, 2.0)] });
        m.insert(3, Node { purchase_price: 0.0, extra_cost: 1.0, composite: true, ingredients: vec![(2, 3.0)] });
        assert_eq!(unit_cost(&m, 3), 61.0);
    }

    #[test]
    fn cycle_detection() {
        let mut m = HashMap::new();
        m.insert(1, leaf(1.0, 0.0));
        m.insert(2, Node { purchase_price: 0.0, extra_cost: 0.0, composite: true, ingredients: vec![(1, 1.0)] });
        m.insert(3, Node { purchase_price: 0.0, extra_cost: 0.0, composite: true, ingredients: vec![(2, 1.0)] });
        assert!(creates_cycle(&m, 1, &[3])); // 1 -> 3 -> 2 -> 1
        assert!(creates_cycle(&m, 5, &[5]));
        assert!(!creates_cycle(&m, 3, &[1]));
        // собственный старый состав игнорируется как источник цикла только через замену: 2 -> 3 даст цикл 2->3->2
        assert!(creates_cycle(&m, 2, &[3]));
    }
}
