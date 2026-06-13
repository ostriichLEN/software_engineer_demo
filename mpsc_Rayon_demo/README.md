# Rust Concurrency Ticket Platform Demo

這份專案是大學軟體工程期末報告用的 concurrency / parallel programming demo。主題從單純「多執行緒售票」升級成一個小型演唱會售票平台，讓不同工具各自出現在合理的工程場景中。

核心故事：

```text
Stage 0: Rust compiler safety demos
  mpsc / Rayon 錯誤寫法在編譯期被擋下，展示 ownership、Send、parallel closure safety

Stage 1: C ticket service variants
  unsafe shared counter 展示 race condition；mutex 修正；queue 模式對照 Rust mpsc

Stage 2: Rust mpsc order service
  多個前端節點送訂單到 bounded channel，由單一中央售票服務處理庫存

Stage 3: Rayon batch analytics
  售票結束後，對大量銷售紀錄做 sequential vs parallel 批次分析，並可對照 C OpenMP
```

## 為什麼這樣設計

`mpsc` 和 Rayon 不是同一種工具。

- `mpsc` 適合展示 live system 裡的任務流、message passing、ownership boundary、bounded queue 與 backpressure。
- Rayon 適合展示 CPU-bound batch job，例如營收統計、銷售分類、異常訂單檢查、窗口銷售量排名。

因此本專案保留售票平台這個共同主題，但把 demo 拆成「即時訂單服務」與「售後資料分析」兩個子系統。這比把 Rayon 硬塞進扣庫存流程更貼近軟體工程。

## 專案結構

```text
.
├── c_demo/
│   └── ticket_race.c          # C unsafe / mutex / queue 對照
│   └── openmp_analytics.c     # C OpenMP batch analytics，對照 Rayon
├── docs/
│   └── demo_script_zh.md      # demo 影片錄製腳本
├── safety_demos/              # Rust compile-fail / safe parallel examples
├── scripts/
│   └── run_safety_demos.ps1   # 一次執行安全性 demo
│   └── run_openmp_vs_rayon.ps1 # C OpenMP vs Rust Rayon 對照
├── src/
│   └── main.rs                # Rust mpsc + Rayon platform demo
├── Cargo.toml
└── README.md
```

## 快速執行

### Stage 0：Rust 編譯期安全性 demo

```powershell
powershell -ExecutionPolicy Bypass -File scripts\run_safety_demos.ps1
```

觀察重點：

- `mpsc_use_after_send.rs` 會被 E0382 擋下：資料被送進 channel 後，原本變數不能再被使用。
- `mpsc_non_send_rc.rs` 會被 E0277 擋下：`Rc<T>` 不是 `Send`，不能跨 thread 傳送。
- `rayon_shared_mutation` 會被 E0596 擋下：Rayon parallel closure 不允許任意修改外部共享 `Vec`。
- `rayon_safe_fold` 會成功：使用 `fold/reduce` 建立 thread-local 結果後再合併。

### Stage 1：C race condition 與 producer-consumer queue

```powershell
New-Item -ItemType Directory -Force -Path target
gcc c_demo/ticket_race.c -O2 -pthread -o target/c_ticket_race.exe
.\target\c_ticket_race.exe unsafe 100 16 80
.\target\c_ticket_race.exe mutex 100 16 80
.\target\c_ticket_race.exe queue 100 16 80 32 500
```

觀察重點：

- `unsafe` 通常會出現 `oversold: true` 或 invariant false。
- `mutex` 會修正結果，但 C 端仍靠工程師自己記得保護 critical section。
- `queue` 使用 `pthread_mutex`、`pthread_cond_t` 和手寫 bounded queue，做出和 Rust `mpsc` 類似的 producer-consumer 架構。

### Stage 2 + Stage 3：Rust 完整平台 demo

```powershell
cargo run --release -- all --tickets 100 --producers 16 --orders 80 --queue-capacity 32 --analytics-records 500000
```

只跑 `mpsc` 即時訂單服務：

```powershell
cargo run --release -- mpsc --tickets 100 --producers 16 --orders 80 --queue-capacity 32 --service-delay-us 500
```

只跑 Rayon 售後批次分析：

```powershell
cargo run --release -- rayon --analytics-records 1000000 --producers 16
```

對照 C OpenMP 與 Rust Rayon：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\run_openmp_vs_rayon.ps1 1000000 16
```

手動執行 OpenMP：

```powershell
gcc c_demo/openmp_analytics.c -O2 -fopenmp -o target/c_openmp_analytics.exe
.\target\c_openmp_analytics.exe 1000000 16
```

如果 Windows Defender 或防毒軟體阻擋 MinGW OpenMP 產生的 exe，這通常是本機安全軟體對 OpenMP runtime 的誤判；可以改在 WSL/Linux、MSVC OpenMP 環境，或加入明確允許後再測。

## Rust mpsc demo 展示什麼

Rust `mpsc` 這段可以直接對照 C 的 `queue` 模式：

| 面向 | C queue | Rust mpsc |
|---|---|---|
| 訂單傳遞 | 手寫 circular buffer | 標準函式庫 channel |
| 同步機制 | 手動管理 mutex / condition variable | `sync_channel` 封裝同步細節 |
| 庫存所有權 | 由程式設計者約定 ticket office 擁有 | ownership 與型別系統讓資料流更明確 |
| 風險 | 容易寫錯 lock、signal、close 條件 | 較少手寫同步原語，錯誤面較小 |

Rust `mpsc` 這段模擬多個前端節點同時送訂單：

```text
front-end producer 0 ----\
front-end producer 1 -----\
front-end producer 2 ------> bounded mpsc channel ---> central ticket office
...                       /
front-end producer N ----/
```

中央售票服務是唯一擁有庫存的 consumer，因此多個 producer 不會直接修改同一份票數。

輸出會包含：

- submitted orders
- sold / rejected sold out
- regular / VIP sold
- remaining inventory
- business invariant 是否成立
- producer sends delayed by backpressure
- total producer send wait

其中 `--queue-capacity` 可以展示 bounded queue。容量越小、`--service-delay-us` 越大，producer 越容易因 backpressure 等待。

## Rayon 與 OpenMP demo 展示什麼

Rayon 這段模擬售票結束後的批次報表：

```text
sales records
  -> revenue
  -> regular / VIP count
  -> high value sales
  -> fraud/manual review candidates
  -> busiest source
  -> checksum
```

程式會同時計算：

- sequential analytics
- Rayon parallel analytics

並印出：

- sequential elapsed
- parallel elapsed
- speedup
- sequential result equals parallel result

注意：不同電腦、資料量、CPU core 數會影響 Rayon 是否明顯更快。錄影時可以把 `--analytics-records` 調高，例如 `1000000` 或 `2000000`。

C OpenMP 的 `c_demo/openmp_analytics.c` 做同樣型態的批次分析。它的比較重點是：

| 面向 | C OpenMP | Rust Rayon |
|---|---|---|
| 平行化方式 | `#pragma omp parallel for` / reduction | `par_iter` / `fold` / `reduce` |
| 程式碼風格 | 在迴圈上加 compiler directive | 使用 Rust iterator API |
| 資料安全 | 需要開發者正確設計 reduction / shared variables | closure trait、ownership、Send/Sync 會參與編譯期檢查 |
| 適合展示 | 傳統 C/C++ HPC 平行迴圈 | Rust 資料平行 API 與型別安全 |

## 報告可以使用的工程結論

- Race condition 是 business invariant 被破壞，不只是程式碼「跑起來怪怪的」。
- C 可以用 mutex 修正，但保護 shared mutable state 的責任主要落在工程師紀律。
- C 也可以手寫 bounded queue 做出類似 Rust `mpsc` 的架構，但需要自行維護 circular buffer、mutex、condition variable 與 close 條件。
- `mpsc` 把多個 producers 與單一 consumer 串起來，讓庫存 ownership 集中在中央服務。
- Rust `mpsc` 會結合 ownership 與 `Send` bound，讓錯誤的跨 thread 資料傳遞在編譯期被擋下。
- bounded channel 可以表現 backpressure，這是高併發系統常見的流量控制設計。
- Rayon 適合互相獨立的資料處理，用 `par_iter`、`fold`、`reduce` 把 batch analytics 分散到多核心 CPU。
- Rayon 的 parallel iterator API 會避免 closure 任意修改外部共享可變資料，促使開發者使用 `fold/reduce` 這類可安全合併的模式。
- Rayon 的合理性能對照是 OpenMP，因為兩者都處理 CPU-bound data parallelism；`mpsc` 則是另一類 live message-passing 問題。
- `mpsc` 是 live task flow；Rayon 是 batch data parallelism。兩者同屬並行程式設計，但解決的軟體工程問題不同。

## 參考資料

- Rust Book 第 16 章，無懼並行：https://rust-lang.tw/book-tw/ch16-00-concurrency.html
- Rust `std::sync::mpsc` 官方文件：https://doc.rust-lang.org/std/sync/mpsc/
- Rayon 官方 docs.rs 文件：https://docs.rs/rayon/
- 使用者提供的 iThome 參考文章：https://ithelp.ithome.com.tw/m/articles/10376807
- 使用者提供的 iThome 參考文章：https://ithelp.ithome.com.tw/articles/10367508
