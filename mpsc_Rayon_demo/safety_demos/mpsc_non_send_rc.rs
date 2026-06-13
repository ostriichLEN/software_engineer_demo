use std::rc::Rc;
use std::sync::mpsc;
use std::thread;

fn main() {
    let (tx, rx) = mpsc::channel::<Rc<String>>();

    thread::spawn(move || {
        tx.send(Rc::new(String::from("VIP ticket order"))).unwrap();
    });

    println!("{}", rx.recv().unwrap());
}

