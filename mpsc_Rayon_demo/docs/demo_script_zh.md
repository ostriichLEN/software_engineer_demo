# 期末報告 Demo 腳本：Rust Concurrency Programming

## Demo 主題

本 demo 用「高併發演唱會售票平台」說明 concurrency 與 parallelism 在軟體工程中的不同角色。

一句話版本：

> C unsafe counter 展示 race condition；C queue 與 Rust mpsc 對照即時訂單服務；Rayon 展示售後批次分析。

## 開場講法

今天的需求不是只寫一個會扣票的程式，而是設計一個小型售票平台。

我們會看三個階段：

1. Rust 編譯器如何擋下錯誤的 mpsc / Rayon 並行寫法。
2. C 的共享變數造成超賣。
3. C 用 mutex 修正共享 counter，再用手寫 queue 做出 producer-consumer 架構。
4. Rust 用 `mpsc` 把多個前端訂單送到中央售票服務，對照 C queue 的工程負擔。
5. 售票結束後，用 Rayon 對大量銷售紀錄做平行分析。
6. 最後用 OpenMP 對照 Rayon，說明兩者都是資料平行工具。

系統必須維持兩個 business invariants：

```text
sold + remaining == initial_inventory
sold + rejected == submitted_orders
```

## Part 0：Rust 編譯期安全性 demo

執行：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\run_safety_demos.ps1
```

講解：

- 第一個失敗案例會出現 E0382：`String` 被送進 `mpsc` channel 後，原變數不能再使用，因為 ownership 已經移動。
- 第二個失敗案例會出現 E0277：`Rc<String>` 不是 `Send`，Rust 不允許它跨 thread 傳送。
- 第三個失敗案例會出現 E0596：Rayon parallel closure 不允許直接修改外部共享 `Vec`。
- 最後一個成功案例使用 Rayon `fold/reduce`，每個 worker 先累積 local result，再安全合併。

補充講法：

> 這些不是效能數字，而是 Rust 相對 C 手寫 pthread queue 的重要優勢：很多可能造成 data race 或 ownership bug 的寫法，在執行前就被編譯器擋下。

## Part 1：C unsafe race condition

執行：

```powershell
New-Item -ItemType Directory -Force -Path target
gcc c_demo/ticket_race.c -O2 -pthread -o target/c_ticket_race.exe
.\target\c_ticket_race.exe unsafe 100 16 80
```

講解：

- 多個 thread 同時讀寫 `tickets_left`。
- 程式刻意在讀取與寫回之間加入 delay，讓 race condition 更容易被觀察到。
- 如果輸出出現 `oversold: true` 或 invariant false，就代表系統賣出的票數已經不可信。

接著執行：

```powershell
.\target\c_ticket_race.exe mutex 100 16 80
.\target\c_ticket_race.exe queue 100 16 80 32 500
```

講解：

- `pthread_mutex` 把查票與扣票包成 critical section。
- 這可以修正 race condition。
- 但 C 不會強制每個共享狀態都必須被保護，安全性仰賴工程師紀律。
- `queue` 模式把多個 producer 的訂單送進手寫 bounded queue，再由單一 ticket office thread 處理庫存。
- 這個模式和 Rust `mpsc` 是真正的架構對照：兩者都讓單一 consumer 擁有庫存，但 C 需要手動處理 mutex、condition variable、queue close 條件與記憶體配置。

## Part 2：Rust mpsc 即時訂單服務

執行：

```powershell
cargo run --release -- mpsc --tickets 100 --producers 16 --orders 80 --queue-capacity 32 --service-delay-us 500
```

講解：

- `producers` 代表多個前端節點或售票窗口。
- 每個 producer 把 `Order` 送進 bounded mpsc channel。
- 中央 ticket office 是唯一 consumer，也是唯一擁有庫存的地方。
- 多個 producer 不直接改庫存，因此 race condition 被架構邊界隔離。

錄影時可以指輸出中的幾個欄位：

```text
submitted orders
sold
rejected/sold out
remaining by tier
invariant ...
producer sends delayed by backpressure
```

補充講法：

> 這裡 mpsc 的重點不是速度，而是 live system 中的訊息傳遞與 ownership boundary。C queue 和 Rust mpsc 做的是類似架構，但 Rust 的標準 channel 幫我們封裝很多同步細節；bounded channel 還能讓我們觀察 backpressure，也就是中央服務處理不夠快時，producer 會被迫等待。

可以再跑一次小 queue：

```powershell
cargo run --release -- mpsc --tickets 100 --producers 16 --orders 80 --queue-capacity 1 --service-delay-us 500
```

觀察 `producer sends delayed by backpressure` 是否變多。

## Part 3：Rayon 售後批次分析，並對照 OpenMP

執行：

```powershell
cargo run --release -- rayon --analytics-records 1000000 --producers 16
```

講解：

- 售票結束後，系統要產生報表。
- 每一筆銷售紀錄大多可以獨立分析。
- Rayon 用 `par_iter`、`fold`、`reduce` 把資料切到多個 CPU core 上處理。

輸出重點：

```text
sequential elapsed
parallel elapsed
speedup
sequential result equals parallel result
revenue
fraud/manual review candidates
busiest source
```

補充講法：

> Rayon 適合 batch analytics，不適合硬拿來做共享庫存扣減。扣庫存有順序與 ownership 問題；銷售紀錄分析則是大量獨立資料處理，這才是 Rayon 的自然場景。

如果 speedup 不明顯，可以加大資料量：

```powershell
cargo run --release -- rayon --analytics-records 2000000 --producers 16
```

接著執行 OpenMP / Rayon 對照：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\run_openmp_vs_rayon.ps1 1000000 16
```

講解：

- OpenMP 是 C/C++ 常見的資料平行工具，通常用 `#pragma omp parallel for` 讓迴圈平行化。
- Rayon 是 Rust 的資料平行工具，用 `par_iter`、`fold`、`reduce` 寫成 iterator pipeline。
- 兩者比較的是 batch analytics performance，不是即時訂單流程。
- 如果 Windows Defender 阻擋 OpenMP exe，報告時要說明這是本機安全軟體對 MinGW OpenMP binary 的執行阻擋，不是 OpenMP 程式碼設計失敗。

## Part 4：完整串接

執行：

```powershell
cargo run --release -- all --tickets 100 --producers 16 --orders 80 --queue-capacity 32 --analytics-records 500000
```

講解：

- Rust CLI 的 `all` 會提醒先看 C counter。
- 接著跑 mpsc 即時訂單服務。
- 最後用 live sale records 作為種子，擴充成大量 historical sales records 交給 Rayon 做 batch analytics。

## 結尾講法

這個 demo 的結論是：

```text
C unsafe shared counter -> 會破壞售票 invariant
C queue ticket office -> 用手寫 pthread queue 達到 message passing 架構
Rust compile-time checks -> ownership、Send、Rayon closure safety 直接擋下錯誤
Rust mpsc order service -> 用 message passing 與 ownership boundary 管理即時訂單
Rayon batch analytics -> 用資料平行加速大量售後統計
OpenMP analytics -> C/C++ 常見資料平行對照組
```

所以 `mpsc` 和 Rayon 都是 Rust 並行程式設計的重要工具，但它們應該放在不同的工程問題裡展示。
