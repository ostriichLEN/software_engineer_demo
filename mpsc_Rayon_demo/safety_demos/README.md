# Rust Safety Demos

這個資料夾用來展示 Rust 在 concurrency / parallel programming 中比較特別的優勢：有些錯誤不是靠執行期測試才發現，而是直接在編譯期被擋下。

## Demo 1：mpsc 送出後不能再使用原值

```powershell
rustc safety_demos/mpsc_use_after_send.rs
```

預期結果：編譯失敗。`String` 被 `send` 移入 channel 後，原本的變數不能再被使用。這展示 `mpsc` 和 ownership 的結合：傳訊息同時也是資料所有權轉移。

## Demo 2：非 Send 型別不能跨 thread 傳送

```powershell
rustc safety_demos/mpsc_non_send_rc.rs
```

預期結果：編譯失敗。`Rc<T>` 不是 thread-safe reference counting，因此不能透過 thread/channel 跨執行緒傳送。

## Demo 3：Rayon 不允許在 parallel closure 中任意修改共享 Vec

```powershell
cargo check --offline --manifest-path safety_demos/rayon_shared_mutation/Cargo.toml
```

預期結果：編譯失敗。Rayon 的 `for_each` closure 不能安全地同時修改外部共享 `Vec`。

## Demo 4：Rayon 正確寫法，使用 fold/reduce 合併局部結果

```powershell
cargo run --release --offline --manifest-path safety_demos/rayon_safe_fold/Cargo.toml
```

預期結果：編譯成功並輸出統計結果。每個 worker 先建立自己的 local vector，最後用 `reduce` 合併，避免共享可變狀態。

## 一次執行全部

```powershell
.\scripts\run_safety_demos.ps1
```

前三個案例應該失敗，最後一個案例應該成功。這裡的重點是：失敗不是壞事，而是 Rust 在編譯期阻止了可能導致 data race 或 ownership bug 的程式。
