# 實驗結果整理：Rust Concurrency Ticket Platform Demo

## 1. 實驗目的

本實驗以「高併發演唱會售票平台」作為案例，先展示 Rust 在編譯期能阻止哪些並行程式錯誤，再用 C 語言的共享變數售票程式展示 race condition 可能造成的超賣問題，最後觀察 Rust 在並行程式設計中的兩種不同工具：

- `std::sync::mpsc`：用於即時訂單處理，展示 message passing、bounded queue 與 backpressure。
- Rayon：用於售後批次分析，展示資料平行處理與 sequential / parallel 效能比較。

需要特別說明的是，Rust 的優勢不只體現在執行結果，也體現在編譯期約束。`mpsc` 會結合 ownership 與 `Send` 檢查，Rayon 的 parallel iterator 也會限制不安全的共享可變捕捉。C 實驗則作為 runtime 問題動機與架構 baseline：它證明未保護的共享可變狀態會破壞售票系統的 business invariant，並且手寫 producer-consumer queue 需要開發者自行維護同步細節。

## 2. 實驗環境與共同設定

Rust 程式皆使用 release mode 執行：

```powershell
cargo run --release -- ...
```

共同售票設定如下：

| 參數 | 數值 | 含意 |
|---|---:|---|
| `tickets` | 100 | 初始庫存票數 |
| `producers` | 16 | 同時送出訂單的前端節點或售票窗口數 |
| `orders` | 80 | 每個 producer 送出的訂單數 |
| `total_orders` | 1280 | 總訂單數，等於 `16 * 80` |
| `queue_capacity` | 32 | `mpsc` bounded channel 容量 |
| `service-delay-us` | 500 | 中央售票服務處理每筆訂單的模擬延遲 |

系統需要維持的 business invariant：

```text
sold + remaining == initial_inventory
sold + rejected == submitted_orders
```

第一個 invariant 用來確認沒有超賣；第二個 invariant 用來確認所有訂單最後都被歸類為成功售出或售罄拒絕。

## 3. 實驗零：Rust 編譯期安全性優勢

### 3.1 執行指令

```powershell
powershell -ExecutionPolicy Bypass -File scripts\run_safety_demos.ps1
```

也可以分開執行：

```powershell
rustc safety_demos/mpsc_use_after_send.rs
rustc safety_demos/mpsc_non_send_rc.rs
cargo check --offline --manifest-path safety_demos/rayon_shared_mutation/Cargo.toml
cargo run --release --offline --manifest-path safety_demos/rayon_safe_fold/Cargo.toml
```

### 3.2 實驗數據

| 案例 | 預期結果 | 實際觀察 | 含意 |
|---|---|---|---|
| `mpsc_use_after_send.rs` | 編譯失敗 | E0382：borrow of moved value | 資料送進 channel 後 ownership 移動，原變數不能再使用 |
| `mpsc_non_send_rc.rs` | 編譯失敗 | E0277：`Rc<String>` cannot be sent between threads safely | 非 `Send` 型別不能跨 thread 傳送 |
| `rayon_shared_mutation` | 編譯失敗 | E0596：cannot borrow captured variable as mutable | Rayon 不允許 parallel closure 直接修改外部共享 `Vec` |
| `rayon_safe_fold` | 編譯成功 | `total sales: 100000`，每個 source 為 6250 | 使用 `fold/reduce` 建立 local result 後安全合併 |

### 3.3 步驟含意

這組實驗不是測速度，而是測「錯誤是否能在執行前被阻止」。在 C 的 pthread queue 中，資料所有權、是否能跨 thread、是否可以同時修改共享資料，主要都靠程式設計者自行維護；Rust 則把一部分規則提升到型別系統和編譯器檢查。

`mpsc_use_after_send.rs` 展示 `send` 不只是把值複製到 queue，而是把值的 ownership 移入 channel。這能避免「送出後又在原 thread 使用同一份資料」造成資料生命週期混亂。

`mpsc_non_send_rc.rs` 展示 `Send` bound。`Rc<T>` 是非 thread-safe reference counting，如果讓它跨 thread，可能造成 reference count data race。Rust 會在 `thread::spawn` 階段直接拒絕這個程式。

`rayon_shared_mutation` 展示 Rayon parallel iterator 的 closure safety。多個 worker 同時修改同一個外部 `Vec` 是典型 data race 風險；Rust/Rayon 不允許這種寫法通過編譯。

`rayon_safe_fold` 展示正確模式：每個 worker 先使用自己的 local vector 累積結果，最後再用 `reduce` 合併。這也是主程式 Rayon analytics 使用的設計。

### 3.4 觀察重點

- 這三個錯誤案例都沒有進入執行期；它們在編譯期就被擋下。
- E0382 對應 ownership transfer，能展示 `mpsc` 傳訊息和所有權移動是同一件事。
- E0277 對應 `Send` trait，能展示 Rust 不允許非 thread-safe 型別跨 thread。
- E0596 對應 Rayon parallel closure 對共享可變狀態的限制。
- 成功案例不是用 lock 包住共享 vector，而是使用 Rayon 推薦的 `fold/reduce`，讓平行處理自然形成「local compute, global merge」。

### 3.5 與 C 實驗的關係

這組實驗補上了 C 對照較難呈現的部分：C 可以透過 pthread 寫出 mutex 或 bounded queue，但 C 編譯器不會理解「這份資料送進 queue 後不能再用」、「這個型別不能跨 thread」、「parallel worker 不該同時修改同一個 Vec」。這些通常需要 code review、測試、sanitizer 或開發者紀律。

因此 Rust 的優勢不是「完全不需要同步」，而是把 ownership、thread-safety trait bound、parallel iterator closure safety 變成編譯期規則，讓錯誤更早暴露。

## 4. 實驗一：C 三種售票設計對照

### 4.1 執行指令

```powershell
.\target\c_ticket_race.exe unsafe 100 16 80
.\target\c_ticket_race.exe mutex 100 16 80
.\target\c_ticket_race.exe queue 100 16 80 32 500
```

### 4.2 實驗數據

| 指標 | C unsafe shared counter | C mutex shared counter | C queue ticket office |
|---|---:|---:|---:|
| tickets | 100 | 100 | 100 |
| threads / producers | 16 | 16 | 16 |
| attempts / orders per thread | 80 | 80 | 80 |
| total requests / orders | 1280 | 1280 | 1280 |
| queue capacity | - | - | 32 |
| service delay per order | - | - | 500 us |
| sold | 1048 | 100 | 100 |
| failed / rejected sold out | 232 | 1180 | 1180 |
| remaining | 8 | 0 | 0 |
| sold by tier | - | - | regular=80, vip=20 |
| remaining by tier | - | - | regular=0, vip=0 |
| invariant 是否成立 | false | true | true |
| oversold | true | false | false |
| producer sends delayed by backpressure | - | - | 1193 |
| cumulative producer send wait | - | - | 10.858705s |
| elapsed | - | - | 0.749304s |

補充：C unsafe 的 race condition 具有非決定性，不同次執行可能出現不同錯誤數字；重點不是固定得到 `sold = 1048`，而是 invariant 會被破壞，且 `sold` 可能超過初始庫存 100。C queue 與 Rust mpsc 的時間、backpressure 次數也會受 OS 排程與 CPU 負載影響，表格記錄的是一次執行觀察值。

### 4.3 步驟含意

這組實驗使用同樣的售票需求，但刻意比較三種 C 實作：

- `unsafe`：多個 thread 直接讀寫同一個全域變數 `tickets_left`，沒有保護 critical section。
- `mutex`：使用 `pthread_mutex` 包住檢查庫存與扣票的區段，讓同一時間只有一個 thread 能修改票數。
- `queue`：多個 producer 將訂單送入 bounded queue，由單一 ticket office thread 擁有並修改庫存。

在 `unsafe` 版本中，程式會先讀取 `tickets_left`，再經過一小段 delay 後寫回扣票結果。這會放大 race condition：多個 thread 可能同時看到「還有票」，於是各自都完成售票，造成同一份庫存被重複賣出。

在 `queue` 版本中，C 程式手動實作 circular buffer、`pthread_mutex_t`、`pthread_cond_t`、queue close 條件與 producer/consumer 同步。這個模式和 Rust `mpsc` 的架構最接近，兩者都是「多個 producer 送訂單，單一 consumer 擁有庫存」。

### 4.4 觀察重點

- C unsafe 的 `sold = 1048`，但初始票數只有 100，表示系統嚴重超賣。
- C unsafe 的 invariant 為 false，代表這不是單純效能或輸出格式問題，而是售票需求本身被破壞。
- C mutex 的 `sold = 100`、`failed = 1180`、`remaining = 0`，符合 100 張票、1280 筆請求的合理結果。
- C mutex 能修正 race condition，但它依賴開發者正確找出所有 critical section 並手動加鎖。
- C queue 的 `sold = 100`、`rejected = 1180`、`remaining = 0`，和 Rust `mpsc` 一樣維持售票 invariant。
- C queue 的 backpressure 次數為 1200，表示 bounded queue 容量有限時，producer 會因 queue 滿而等待。
- C queue 能做出和 Rust `mpsc` 類似的架構，但實作上需要大量手動同步程式碼，工程風險較高。

### 4.5 與 Rust 實驗的關係

重新設計後，C 實驗可以和 Rust 形成更清楚的對照關係：

- C unsafe 對照 C mutex：說明同樣是共享 counter，有無 critical section 會直接影響 correctness。
- C mutex 對照 C queue：說明除了幫共享 counter 加鎖，也可以改用 producer-consumer 架構讓單一 owner 管理庫存。
- C queue 對照 Rust `mpsc`：兩者架構相似，但 C 需要手寫 bounded queue 與同步細節；Rust 直接使用標準函式庫 channel，資料流與 ownership 更明確。
- Rayon 則不是用來修正扣庫存 race condition，而是展示售票平台另一個子系統，也就是售後批次分析。

因此報告時可以說：「C unsafe 展示錯誤，C mutex 展示手動加鎖修正，C queue 展示手寫 message passing 架構，Rust `mpsc` 則用語言與標準函式庫提供更乾淨的 message-passing 寫法。」

## 5. 實驗二：mpsc 即時訂單服務

### 5.1 執行指令

```powershell
cargo run --release -- mpsc --tickets 100 --producers 16 --orders 80 --queue-capacity 32 --service-delay-us 500
```

### 5.2 實驗數據

| 指標 | 數值 |
|---|---:|
| 初始票數 | 100 |
| producer 數量 | 16 |
| 每個 producer 訂單數 | 80 |
| 總訂單數 | 1280 |
| queue capacity | 32 |
| 每筆訂單處理延遲 | 500 us |
| submitted orders | 1280 |
| producer submitted count | 1280 |
| sold | 100 |
| rejected / sold out | 1180 |
| regular sold | 80 |
| VIP sold | 20 |
| regular remaining | 0 |
| VIP remaining | 0 |
| invariant 是否成立 | true |
| producer sends delayed by backpressure | 1246 |
| cumulative producer send wait | 21.3242781s |
| elapsed | 1.4136108s |

### 5.3 步驟含意

此實驗模擬 16 個前端節點同時送出訂單，所有訂單先進入 bounded `mpsc` channel，再由單一中央售票服務依序處理庫存。

```text
producer threads -> bounded mpsc channel -> central ticket office
```

這個設計的核心是「讓中央售票服務擁有庫存」。producer 不直接修改庫存，因此系統不需要讓多個 thread 同時讀寫同一份票數。這和單純用多個 thread 共同扣同一個 counter 的設計不同，`mpsc` 把共享可變狀態轉換成訊息傳遞與所有權邊界。

### 5.4 觀察重點

- `sold = 100` 且 `remaining = 0`，表示所有庫存都被售出，但沒有超過 100 張。
- `rejected / sold out = 1180`，表示剩下的訂單被明確歸類為售罄拒絕，而不是造成錯誤售票。
- `sold + remaining == initial` 成立，代表沒有超賣。
- `sold + rejected == submitted` 成立，代表所有訂單都有被處理。
- `producer sends delayed by backpressure = 1246`，表示大多數送出動作都曾因 queue 容量有限或中央服務處理速度較慢而等待。
- `cumulative producer send wait = 21.3242781s` 大於整體 `elapsed = 1.4136108s`，是因為多個 producer thread 的等待時間是累加值；不同 thread 的等待會在真實時間中重疊發生。

### 5.5 與 C queue 的對照

| 指標 | C queue | Rust mpsc |
|---|---:|---:|
| submitted orders | 1280 | 1280 |
| sold | 100 | 100 |
| rejected / sold out | 1180 | 1180 |
| invariant 是否成立 | true | true |
| producer sends delayed by backpressure | 1193 | 1246 |
| elapsed | 0.749304s | 1.4136108s |

這張表不是要宣稱 C 或 Rust 在效能上絕對較快，因為兩者的 delay 實作、runtime、計時方式與同步封裝都不同。更重要的觀察是：兩者使用相同的 producer-consumer 架構後，都能維持售票 invariant；差別在於 C queue 必須手動實作同步細節，而 Rust `mpsc` 使用標準 channel 表達同樣的資料流。

## 6. 實驗三：Rayon 批次分析，1,000,000 筆資料

### 6.1 執行指令

```powershell
cargo run --release -- rayon --analytics-records 1000000 --producers 16
```

### 6.2 實驗數據

| 指標 | 數值 |
|---|---:|
| analytics records | 1,000,000 |
| sequential elapsed | 101.6667ms |
| parallel elapsed | 15.4698ms |
| speedup | 6.57x |
| sequential result equals parallel result | true |
| revenue | $16,714,463.00 |
| regular sales | 857,136 |
| VIP sales | 142,864 |
| high value sales | 142,864 |
| fraud / manual review candidates | 20,117 |
| busiest source | #15 |
| busiest source sales | 62,500 |
| checksum | 5346 |

### 6.3 步驟含意

此實驗模擬售票結束後的批次資料分析。每一筆銷售紀錄都可以被獨立處理，例如計算營收、票種數量、高價票數、人工審查候選訂單與來源統計。

Rayon 使用資料平行的概念，將原本 sequential 的迭代分析拆分到多個 CPU core 上執行：

```text
sales records -> par_iter -> fold -> reduce -> analytics result
```

此處 Rayon 的重點是加速 CPU-bound batch job，而不是處理即時訂單流程或共享庫存扣減。

### 6.4 觀察重點

- sequential 與 parallel 結果完全一致，表示平行化沒有改變分析結果。
- 1,000,000 筆資料下，Rayon parallel elapsed 為 15.4698ms，明顯低於 sequential 的 101.6667ms。
- speedup 達 6.57x，代表資料量足夠時，Rayon 可以有效利用多核心 CPU。
- busiest source 為 #15，且銷售數為 62,500，剛好是 `1,000,000 / 16`，表示資料平均分散到 16 個來源。

## 7. 實驗四：Rayon 批次分析，2,000,000 筆資料

### 7.1 執行指令

```powershell
cargo run --release -- rayon --analytics-records 2000000 --producers 16
```

### 7.2 實驗數據

| 指標 | 數值 |
|---|---:|
| analytics records | 2,000,000 |
| sequential elapsed | 201.5869ms |
| parallel elapsed | 25.6605ms |
| speedup | 7.86x |
| sequential result equals parallel result | true |
| revenue | $33,428,926.25 |
| regular sales | 1,714,272 |
| VIP sales | 285,728 |
| high value sales | 285,728 |
| fraud / manual review candidates | 39,912 |
| busiest source | #15 |
| busiest source sales | 125,000 |
| checksum | 25158103 |

### 7.3 觀察重點

- 資料量從 1,000,000 筆增加到 2,000,000 筆後，sequential elapsed 從 101.6667ms 增加到 201.5869ms，幾乎接近線性成長。
- parallel elapsed 從 15.4698ms 增加到 25.6605ms，雖然也增加，但增加幅度較小。
- speedup 從 6.57x 提升到 7.86x，表示資料量越大，Rayon 的平行化成本越容易被攤平。
- sequential result equals parallel result 仍為 true，代表平行化後仍保持計算正確性。

## 8. 實驗五：Rayon 與 OpenMP 對照設計

### 8.1 執行指令

```powershell
gcc c_demo/openmp_analytics.c -O2 -fopenmp -o target/c_openmp_analytics.exe
.\target\c_openmp_analytics.exe 1000000 16
cargo run --release -- rayon --analytics-records 1000000 --producers 16
```

或使用整合腳本：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\run_openmp_vs_rayon.ps1 1000000 16
```

### 8.2 對照目的

Rayon 的合理比較對象是 OpenMP，而不是 `mpsc`。原因是 Rayon 和 OpenMP 都是資料平行工具，適合處理大量互相獨立的 CPU-bound 工作，例如售後銷售紀錄分析。

本專案的 OpenMP 程式 `c_demo/openmp_analytics.c` 使用和 Rayon 相同型態的資料集與統計項目：

- revenue
- regular / VIP count
- high value sales
- fraud / manual review candidates
- busiest source
- checksum

### 8.3 比較重點

| 面向 | C OpenMP | Rust Rayon |
|---|---|---|
| 平行化方式 | `#pragma omp parallel for` 與 reduction | `par_iter`、`fold`、`reduce` |
| 資料切分 | 由 OpenMP runtime 切分迴圈 | 由 Rayon work-stealing thread pool 切分 iterator |
| 共享狀態處理 | 開發者要明確設計 reduction 或 thread-local buffer | API 鼓勵 local fold，再 reduce 合併 |
| 編譯期安全 | C 編譯器較難阻止不安全 shared mutation | Rust/Rayon 會拒絕不安全的 captured mutable state |
| 報告定位 | 傳統 C/C++ HPC 平行迴圈 | Rust 資料平行與型別安全結合 |

### 8.4 本機執行狀況

在目前 Windows / MinGW 環境中，OpenMP 程式可以成功編譯：

```powershell
gcc c_demo/openmp_analytics.c -O2 -fopenmp -o target/c_openmp_analytics.exe
```

但執行時被 Windows Defender / 安全軟體阻擋：

```text
程式無法執行: 因為檔案包含病毒或潛在的垃圾軟體，所以作業未順利完成。
```

因此本次報告不應假造 OpenMP performance 數據。比較時可以採用以下說法：

- Rayon 的 performance 數據已在本機成功取得。
- OpenMP 對照程式已完成，設計上與 Rayon 做同類型 batch analytics。
- 若要取得 OpenMP 實測數據，建議改在 WSL/Linux、MSVC OpenMP 環境，或將該 exe 加入本機安全軟體允許清單後再測。

這個限制不影響本實驗的設計結論：Rayon 應該和 OpenMP 比較，因為兩者都是資料平行工具；`mpsc` 則應該和 C pthread queue / producer-consumer 架構比較。

## 9. 實驗六：完整 all 流程

### 9.1 執行指令

```powershell
cargo run --release -- all --tickets 100 --producers 16 --orders 80 --queue-capacity 32 --analytics-records 500000
```

### 9.2 mpsc 即時訂單服務數據

| 指標 | 數值 |
|---|---:|
| submitted orders | 1280 |
| producer submitted count | 1280 |
| sold | 100 |
| rejected / sold out | 1180 |
| regular sold | 80 |
| VIP sold | 20 |
| regular remaining | 0 |
| VIP remaining | 0 |
| invariant 是否成立 | true |
| producer sends delayed by backpressure | 1247 |
| cumulative producer send wait | 20.0003366s |
| elapsed | 1.3245338s |

### 9.3 Rayon 批次分析數據

| 指標 | 數值 |
|---|---:|
| analytics records | 500,000 |
| sequential elapsed | 59.0432ms |
| parallel elapsed | 16.9455ms |
| speedup | 3.48x |
| sequential result equals parallel result | true |
| revenue | $8,357,442.50 |
| regular sales | 428,556 |
| VIP sales | 71,444 |
| high value sales | 71,444 |
| fraud / manual review candidates | 10,096 |
| busiest source | #0 |
| busiest source sales | 31,279 |
| checksum | 3316827 |

### 9.4 步驟含意

`all` 模式把 Rust 平台 demo 串起來。實際錄製時，建議先執行 C 的三種模式：`unsafe`、`mutex`、`queue`。這樣可以先看到錯誤共享 counter、手動加鎖修正、手寫 producer-consumer queue 三種設計，再接著執行 Rust `mpsc` 即時訂單服務與 Rayon 批次分析。

這個流程能清楚區分：

- C unsafe shared counter：展示錯誤的 shared mutable state 可能破壞售票需求。
- C mutex shared counter：展示手動 critical section 可以修正超賣。
- C queue ticket office：展示 C 也能做 message passing 架構，但需要手寫 queue 與同步控制。
- Rust `mpsc`：展示標準 channel 如何表達即時訂單處理與 backpressure。
- Rayon：展示大量售後資料的平行分析。

## 10. Rayon 資料量比較

| 資料筆數 | Sequential | Parallel | Speedup |
|---:|---:|---:|---:|
| 500,000 | 59.0432ms | 16.9455ms | 3.48x |
| 1,000,000 | 101.6667ms | 15.4698ms | 6.57x |
| 2,000,000 | 201.5869ms | 25.6605ms | 7.86x |

觀察：

- Sequential 執行時間隨資料量增加而明顯上升。
- Parallel 執行時間也會上升，但上升幅度相對較小。
- 資料量越大，Rayon 的 thread pool、工作切分與 reduce 成本越容易被大量運算攤平，因此 speedup 更明顯。
- 500,000 筆時 speedup 為 3.48x，2,000,000 筆時提升到 7.86x，代表 Rayon 更適合有足夠資料量的 CPU-bound 批次工作。

## 11. 實驗結論

本實驗可以得到以下結論：

1. Rust 的 `mpsc` 與 Rayon 能把部分並行錯誤提前到編譯期。

   `mpsc_use_after_send.rs`、`mpsc_non_send_rc.rs` 與 `rayon_shared_mutation` 都在編譯期被拒絕，分別對應 ownership transfer、`Send` bound 與 parallel closure 不可任意修改共享可變資料。這些錯誤在 C pthread 程式中通常要靠 code review、測試或執行期工具才比較容易發現。

2. C unsafe、C mutex、C queue 形成清楚的工程設計對照。

   C unsafe 直接修改共享 counter，會破壞售票 invariant；C mutex 用 critical section 修正共享 counter；C queue 則改成 producer-consumer 架構，讓單一 ticket office 擁有庫存。這三者展示了從錯誤同步、手動加鎖，到架構重設計的演進。

3. Rust `mpsc` 和 C queue 是最直接的對照組。

   兩者都採用「多個 producer 送訂單，單一 consumer 擁有庫存」的設計，因此都能避免多個 thread 同時扣同一份庫存。差別是 C queue 需要自行維護 circular buffer、mutex、condition variable 與 close 條件；Rust `mpsc` 則用標準函式庫 channel 將這些同步細節封裝起來，讓程式更聚焦在訂單流程與 ownership boundary。

4. `mpsc` 適合用於即時系統中的訊息傳遞。

   在售票平台中，多個 producer 不直接修改庫存，而是將訂單送進 bounded channel，由單一中央售票服務擁有並修改庫存。這種設計能把共享可變狀態集中在清楚的 ownership boundary 內，避免多個 thread 同時扣同一份票數。

5. Bounded channel 可以呈現 backpressure。

   實驗中 `producer sends delayed by backpressure` 超過 1200 次，表示當中央售票服務處理速度有限、queue 容量只有 32 時，producer 會被迫等待。這反映真實高併發系統中的流量控制問題：系統不只是要能接收請求，也要能在下游處理速度不足時控制壓力。

6. Rayon 適合售後批次分析，而不是即時扣庫存。

   Rayon 的強項在於將大量彼此獨立的資料處理平行化。本實驗中 revenue、票種統計、異常候選訂單與來源統計都能用 `par_iter`、`fold`、`reduce` 處理，且 parallel result 與 sequential result 完全一致。

7. Rayon 的合理對照組是 OpenMP。

   Rayon 和 OpenMP 都是 data parallelism 工具，適合比較大量資料分析的 sequential / parallel speedup。OpenMP 對照程式已新增為 `c_demo/openmp_analytics.c`，但本機 Windows Defender 阻擋 MinGW OpenMP binary 執行，因此本次不假造 OpenMP 效能數據。後續若在 WSL/Linux 或 MSVC OpenMP 環境執行，就能取得 C OpenMP 與 Rust Rayon 的直接 performance 對照。

8. 資料量越大，Rayon 效益越明顯。

   1,000,000 筆資料時 speedup 為 6.57x，2,000,000 筆資料時 speedup 提升到 7.86x。這表示當資料量足夠大時，平行化成本會被攤平，多核心 CPU 的優勢更容易被觀察到。

9. `mpsc` 與 Rayon 都屬於並行程式設計工具，但解決的問題不同。

   `mpsc` 解決的是 live task flow、message passing 與 ownership boundary；Rayon 解決的是 batch data parallelism。將兩者放在同一個售票平台的不同子系統中，比把兩者硬套在同一個扣票流程中更符合軟體工程實務。

總結來說，本專案現在形成更完整的對照：Rust safety demos 展示編譯期擋錯，C unsafe 展示 race condition 的錯誤後果，C mutex 展示手動加鎖修正，C queue 對照 Rust `mpsc` 的 message passing 架構，OpenMP 對照 Rayon 的資料平行架構。這樣的分組比單純把所有工具放在同一個售票 counter 上比較更合理，也更能凸顯 Rust 在並行程式設計中「執行前先排除一部分錯誤」與「用高階 API 表達平行資料流」的特色。
