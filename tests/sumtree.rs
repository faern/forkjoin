extern crate forkjoin;

use forkjoin::{TaskResult,Fork,ForkPool,AlgoStyle};

#[cfg(test)]
struct Tree {
    value: usize,
    children: Vec<Tree>,
}

#[cfg(test)]
fn create_tree() -> Tree {
    Tree {
        value: 100,
        children: vec![Tree {
            value: 250,
            children: vec![
                Tree {
                    value: 500,
                    children: vec![],
                },
                Tree {
                    value: 10,
                    children: vec![],
                }],
        }],
    }
}

#[test]
fn sum_tree() {
    let tree = create_tree();

    let seq_sum = sum_tree_seq(&tree);
    let par_sum = sum_tree_par(&tree, 1);
    assert_eq!(860, seq_sum);
    assert_eq!(seq_sum, par_sum);
}

#[cfg(test)]
fn sum_tree_seq(t: &Tree) -> usize {
    t.value + t.children.iter().fold(0, |acc, t2| acc + sum_tree_seq(t2))
}

#[cfg(test)]
fn sum_tree_par(t: &Tree, nthreads: usize) -> usize {
    let forkpool = ForkPool::with_threads(nthreads);

    let result_port = forkpool.schedule(sum_tree_task, t);
    result_port.recv().unwrap()
}

#[cfg(test)]
fn sum_tree_task(t: &Tree) -> TaskResult<&Tree, usize> {
    let val = t.value;

    if t.children.is_empty() {
        TaskResult::Done(val)
    } else {
        let mut fork_args: Vec<&Tree> = vec![];
        for c in t.children.iter() {
            fork_args.push(c);
        }

        TaskResult::Fork(Fork {
            fun: sum_tree_task,
            args: fork_args,
            join: AlgoStyle::Summa(sum_tree_join, val),
        })
    }
}

#[cfg(test)]
fn sum_tree_join(value: &usize, values: &[usize]) -> usize {
    *value + values.iter().fold(0, |acc, &v| acc + v)
}
