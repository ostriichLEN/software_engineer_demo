use std::sync::mpsc;

fn main() {
    let (tx, rx) = mpsc::channel::<String>();
    let order_id = String::from("order-1001");


    // 將資料送入通道
    tx.send(order_id).unwrap(); // order_id Ownership 在這裡被轉移進 tx 了

    //嘗試讀取已經被 Move 的變數
    println!("sent order: {order_id}");
    println!("received order: {}", rx.recv().unwrap());
}

