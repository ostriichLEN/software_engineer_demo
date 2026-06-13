use rayon::prelude::*;

fn main() {
    let mut per_source_sales = vec![0usize; 16];

    // 啟動平行迭代器，底層會把迴圈拆分給多個 CPU 核心執行
    (0..100_000usize).into_par_iter().for_each(|order_id| {
        let source_id = order_id % 16;
        
        // 多個 Worker Threads 試圖同時修改外層的共享 Vec
        per_source_sales[source_id] += 1; 
    });

    println!("{per_source_sales:?}");
}