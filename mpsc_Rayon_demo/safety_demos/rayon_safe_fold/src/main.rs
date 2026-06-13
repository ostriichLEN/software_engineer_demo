use rayon::prelude::*;

fn main() {
    let per_source_sales = (0..100_000usize)
        .into_par_iter()
        .fold(
            || vec![0usize; 16],
            |mut local_counts, order_id| {
                let source_id = order_id % 16;
                local_counts[source_id] += 1;
                local_counts
            },
        )
        .reduce(
            || vec![0usize; 16],
            |mut left, right| {
                for (index, count) in right.into_iter().enumerate() {
                    left[index] += count;
                }
                left
            },
        );

    println!("total sales: {}", per_source_sales.iter().sum::<usize>());
    println!("per source sales: {per_source_sales:?}");
}

