#![feature(core)]
#![feature(collections)]

extern crate core;
extern crate forkjoin;

use forkjoin::{ForkPool,TaskResult,Fork,JoinStyle};

#[test]
fn search_nqueens() {
    let n: usize = 8;
    let empty = vec![];
    let solutions = nqueens(&empty[..], n);
    let num_solutions = solutions.len();
    println!("sequential nqueens gave {} solutions", num_solutions);

    let forkpool = ForkPool::with_threads(4);

    let par_solutions_port = forkpool.schedule(nqueens_task, (empty, n));

    let mut par_solutions: Vec<Board> = vec![];
    loop {
        match par_solutions_port.recv() {
            Err(..) => break,
            Ok(message) => par_solutions.push(message),
        };
    }
    let num_par_solutions = par_solutions.len();

    assert_eq!(num_par_solutions, num_solutions);
    if n == 8 {
        assert_eq!(num_solutions, 92)
    }
    println!("Found {} solutions to nqueens({}x{})", num_solutions, n, n);

    // Make sure that all solutions match. They might not appear in the same order
    for board in solutions {
        let mut found_equal_board = false;

        for par_board in &par_solutions[..] {
            let mut equal_board = true;
            for (queen, par_queen) in board.iter().zip(par_board.iter()) {
                if queen != par_queen {
                    equal_board = false;
                    break;
                }
            }
            if equal_board {
                found_equal_board = true;
                break;
            }
        }
        assert_eq!(found_equal_board, true);
    };
}

#[cfg(test)]
pub type Queen = usize;
pub type Board = Vec<Queen>;
pub type Solutions = Vec<Board>;

#[cfg(test)]
fn nqueens_task((q, n): (Board, usize)) -> TaskResult<(Board,usize), Board> {
    if q.len() == n {
        TaskResult::Done(q)
    } else {
        let mut fork_args: Vec<(Board, usize)> = vec![];
        for i in 0..n {
            let mut q2 = q.clone();
            q2.push(i);

            if ok(&q2[..]) {
                fork_args.push((q2, n));
            }
        }
        TaskResult::Fork(Fork{fun: nqueens_task, args: fork_args, join: JoinStyle::Search})
    }
}

#[cfg(test)]
fn nqueens(q: &[Queen], n: usize) -> Solutions {
    if q.len() == n && ok(q) {
        return vec![q.to_vec()];
    }
    let mut solutions: Solutions = vec![];
    for i in 0..n {
        let mut q2 = q.to_vec();
        q2.push(i);
        let new_q = &q2[..];

        if ok(new_q) {
            let more_solutions = nqueens(new_q, n);
            solutions.push_all(&more_solutions[..]);
        }
    }
    solutions
}

#[cfg(test)]
fn ok(q: &[usize]) -> bool {
    for (x1, &y1) in q.iter().enumerate() {
        for (x2, &y2) in q.iter().enumerate() {
            if x2 > x1 {
                let xd = x2-x1;
                if y1 == y2 || y1 == y2 + xd || (y2 >= xd && y1 == y2 - xd) {
                    return false;
                }
            }
        }
    }
    true
}
