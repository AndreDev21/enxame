//! Ordenação de mensagens de uma conversa (spec 7.1): ordem topológica por `parents`,
//! desempate por `hlc`, depois por `id`. Função pura: o mesmo conjunto produz a mesma ordem
//! independentemente da ordem de chegada.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
};

use crate::hlc::Hlc;

/// Dados mínimos de uma mensagem para ordenar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderItem {
    pub id: [u8; 32],
    pub hlc: Hlc,
    pub parents: Vec<[u8; 32]>,
}

/// Devolve os ids na ordem de exibição. Pais ausentes do conjunto (ainda não recebidos) são
/// ignorados: não bloqueiam a mensagem.
pub fn order_ids(items: &[OrderItem]) -> Vec<[u8; 32]> {
    let key = |i: &OrderItem| (i.hlc, i.id);
    let by_id: HashMap<[u8; 32], &OrderItem> = items.iter().map(|i| (i.id, i)).collect();
    let mut pending: HashMap<[u8; 32], usize> = HashMap::new();
    let mut children: HashMap<[u8; 32], Vec<&OrderItem>> = HashMap::new();
    for it in items {
        let mut parents: Vec<[u8; 32]> = it
            .parents
            .iter()
            .copied()
            .filter(|p| by_id.contains_key(p) && *p != it.id)
            .collect();
        parents.sort_unstable();
        parents.dedup();
        pending.insert(it.id, parents.len());
        for p in parents {
            children.entry(p).or_default().push(it);
        }
    }
    let mut ready: BinaryHeap<Reverse<(Hlc, [u8; 32])>> = items
        .iter()
        .filter(|i| pending[&i.id] == 0)
        .map(|i| Reverse(key(i)))
        .collect();
    let mut out = Vec::with_capacity(items.len());
    while let Some(Reverse((_, id))) = ready.pop() {
        out.push(id);
        if let Some(kids) = children.get(&id) {
            for k in kids {
                if let Some(n) = pending.get_mut(&k.id) {
                    *n -= 1;
                    if *n == 0 {
                        ready.push(Reverse(key(k)));
                    }
                }
            }
        }
    }
    if out.len() < items.len() {
        // Ciclo (impossível com ids por hash; defesa contra dados corrompidos).
        let placed: std::collections::HashSet<_> = out.iter().copied().collect();
        let mut rest: Vec<_> = items.iter().filter(|i| !placed.contains(&i.id)).collect();
        rest.sort_by_key(|i| key(i));
        out.extend(rest.into_iter().map(|i| i.id));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn id(n: u8) -> [u8; 32] {
        [n; 32]
    }

    fn item(n: u8, ms: u64, parents: &[u8]) -> OrderItem {
        OrderItem {
            id: id(n),
            hlc: Hlc::new(ms, 0),
            parents: parents.iter().map(|p| id(*p)).collect(),
        }
    }

    #[test]
    fn orders_by_hlc_then_id_without_parents() {
        let items = vec![item(3, 10, &[]), item(1, 20, &[]), item(2, 10, &[])];
        assert_eq!(order_ids(&items), vec![id(2), id(3), id(1)]);
    }

    #[test]
    fn parent_comes_before_child_even_with_a_later_hlc() {
        // Relógio do autor do pai estava adiantado: o pai (hlc 100) precede o filho (hlc 5).
        let items = vec![item(2, 5, &[1]), item(1, 100, &[])];
        assert_eq!(order_ids(&items), vec![id(1), id(2)]);
    }

    #[test]
    fn missing_parents_do_not_block() {
        let items = vec![item(2, 5, &[99]), item(3, 6, &[2])];
        assert_eq!(order_ids(&items), vec![id(2), id(3)]);
    }

    #[test]
    fn cycles_fall_back_to_hlc_order() {
        let items = vec![item(1, 2, &[2]), item(2, 1, &[1])];
        assert_eq!(order_ids(&items), vec![id(2), id(1)]);
    }

    /// Gera um DAG: o item i só pode ter pais com índice menor.
    fn dag() -> impl Strategy<Value = Vec<OrderItem>> {
        (1usize..40).prop_flat_map(|n| {
            proptest::collection::vec(
                (
                    0u64..50,
                    proptest::collection::vec(any::<prop::sample::Index>(), 0..3),
                ),
                n,
            )
            .prop_map(move |specs| {
                specs
                    .into_iter()
                    .enumerate()
                    .map(|(i, (ms, ps))| {
                        let mut parents: Vec<[u8; 32]> = if i == 0 {
                            vec![]
                        } else {
                            ps.iter().map(|p| id(p.index(i) as u8)).collect()
                        };
                        parents.sort();
                        parents.dedup();
                        OrderItem {
                            id: id(i as u8),
                            hlc: Hlc::new(ms, 0),
                            parents,
                        }
                    })
                    .collect()
            })
        })
    }

    proptest! {
        #[test]
        fn same_set_same_order_regardless_of_arrival(items in dag(), seed in any::<u64>()) {
            let base = order_ids(&items);
            let mut shuffled = items.clone();
            // Embaralhamento determinístico pela semente.
            let mut s = seed | 1;
            for i in (1..shuffled.len()).rev() {
                s ^= s << 13; s ^= s >> 7; s ^= s << 17;
                shuffled.swap(i, (s % (i as u64 + 1)) as usize);
            }
            prop_assert_eq!(&order_ids(&shuffled), &base);
            // Todo pai presente aparece antes do filho.
            let pos: HashMap<_, _> = base.iter().enumerate().map(|(i, x)| (*x, i)).collect();
            for it in &items {
                for p in &it.parents {
                    prop_assert!(pos[p] < pos[&it.id]);
                }
            }
            prop_assert_eq!(base.len(), items.len());
        }
    }
}
