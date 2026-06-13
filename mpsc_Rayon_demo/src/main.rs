use rayon::prelude::*;
use std::collections::VecDeque;
use std::env;
use std::fmt;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_TICKETS: usize = 100;
const DEFAULT_PRODUCERS: usize = 16;
const DEFAULT_ORDERS_PER_PRODUCER: usize = 80;
const DEFAULT_QUEUE_CAPACITY: usize = 32;
const DEFAULT_ANALYTICS_RECORDS: usize = 500_000;
const DEFAULT_SERVICE_DELAY_MICROS: usize = 500;

#[derive(Debug, Clone, Copy)]
struct Settings {
    tickets: usize,
    producers: usize,
    orders_per_producer: usize,
    queue_capacity: usize,
    analytics_records: usize,
    service_delay_micros: usize,
}

impl Settings {
    fn total_orders(self) -> usize {
        self.producers * self.orders_per_producer
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tickets: DEFAULT_TICKETS,
            producers: DEFAULT_PRODUCERS,
            orders_per_producer: DEFAULT_ORDERS_PER_PRODUCER,
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            analytics_records: DEFAULT_ANALYTICS_RECORDS,
            service_delay_micros: DEFAULT_SERVICE_DELAY_MICROS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    All,
    Mpsc,
    Rayon,
}

impl Mode {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "all" => Some(Self::All),
            "mpsc" => Some(Self::Mpsc),
            "rayon" => Some(Self::Rayon),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TicketTier {
    Regular,
    Vip,
}

impl TicketTier {
    fn price_cents(self) -> u64 {
        match self {
            Self::Regular => 1_200,
            Self::Vip => 3_800,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Ticket {
    id: usize,
    tier: TicketTier,
    price_cents: u64,
}

#[derive(Debug, Clone)]
struct Inventory {
    regular: VecDeque<Ticket>,
    vip: VecDeque<Ticket>,
}

impl Inventory {
    fn pop_for(&mut self, tier: TicketTier) -> Option<Ticket> {
        match tier {
            TicketTier::Regular => self.regular.pop_front(),
            TicketTier::Vip => self.vip.pop_front(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Order {
    id: usize,
    customer_id: usize,
    source_id: usize,
    requested_tier: TicketTier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sale {
    order_id: usize,
    customer_id: usize,
    source_id: usize,
    ticket_id: usize,
    tier: TicketTier,
    price_cents: u64,
}

#[derive(Debug, Clone)]
struct ProducerSummary {
    source_id: usize,
    submitted: usize,
    blocked_sends: usize,
    send_wait_total: Duration,
}

#[derive(Debug, Clone)]
struct OrderServiceReport {
    total_inventory: usize,
    total_orders: usize,
    sold: usize,
    rejected_sold_out: usize,
    regular_sold: usize,
    vip_sold: usize,
    regular_remaining: usize,
    vip_remaining: usize,
    queue_capacity: usize,
    service_delay_micros: usize,
    elapsed: Duration,
    producer_summaries: Vec<ProducerSummary>,
    sales: Vec<Sale>,
}

impl OrderServiceReport {
    fn remaining(&self) -> usize {
        self.regular_remaining + self.vip_remaining
    }

    fn invariant_ok(&self) -> bool {
        self.sold + self.remaining() == self.total_inventory
            && self.sold + self.rejected_sold_out == self.total_orders
            && self.sales.len() == self.sold
    }

    fn blocked_sends(&self) -> usize {
        self.producer_summaries
            .iter()
            .map(|summary| summary.blocked_sends)
            .sum()
    }

    fn submitted_by_producers(&self) -> usize {
        self.producer_summaries
            .iter()
            .map(|summary| summary.submitted)
            .sum()
    }

    fn total_send_wait(&self) -> Duration {
        self.producer_summaries
            .iter()
            .map(|summary| summary.send_wait_total)
            .sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AnalyticsResult {
    records: usize,
    revenue_cents: u64,
    regular_sales: usize,
    vip_sales: usize,
    high_value_sales: usize,
    review_required: usize,
    busiest_source: usize,
    busiest_source_sales: usize,
    per_source_sales: Vec<usize>,
    checksum: u64,
}

impl fmt::Display for AnalyticsResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  records: {}", self.records)?;
        writeln!(f, "  revenue: ${:.2}", self.revenue_cents as f64 / 100.0)?;
        writeln!(
            f,
            "  ticket tiers: regular={}, vip={}",
            self.regular_sales, self.vip_sales
        )?;
        writeln!(f, "  high value sales: {}", self.high_value_sales)?;
        writeln!(
            f,
            "  fraud/manual review candidates: {}",
            self.review_required
        )?;
        writeln!(
            f,
            "  busiest source: #{} ({} sales)",
            self.busiest_source, self.busiest_source_sales
        )?;
        writeln!(f, "  checksum: {}", self.checksum)
    }
}

#[derive(Debug, Clone)]
struct AnalyticsBenchmark {
    sequential: AnalyticsResult,
    parallel: AnalyticsResult,
    sequential_elapsed: Duration,
    parallel_elapsed: Duration,
}

fn main() {
    let (mode, settings) = parse_cli(env::args().skip(1).collect());

    print_banner(settings);

    match mode {
        Mode::All => {
            println!("Stage 1: C race-condition counter is in c_demo/ticket_race.c");
            println!("         Run it before this Rust CLI to show the unsafe oversell bug.");
            println!();

            let report = run_mpsc_order_service(settings);
            print_order_service_report(&report);
            println!();

            let benchmark = run_analytics_benchmark(settings, &report.sales);
            print_analytics_benchmark(&benchmark);
        }
        Mode::Mpsc => {
            let report = run_mpsc_order_service(settings);
            print_order_service_report(&report);
        }
        Mode::Rayon => {
            let benchmark = run_analytics_benchmark(settings, &[]);
            print_analytics_benchmark(&benchmark);
        }
    }
}

fn parse_cli(args: Vec<String>) -> (Mode, Settings) {
    let mut settings = Settings::default();
    let mut mode = Mode::All;
    let mut index = 0;

    if let Some(first) = args.first() {
        if let Some(parsed_mode) = Mode::parse(first) {
            mode = parsed_mode;
            index = 1;
        }
    }

    while index < args.len() {
        match args[index].as_str() {
            "--tickets" => {
                settings.tickets = parse_usize_arg(&args, index, "--tickets");
                index += 2;
            }
            "--producers" | "--workers" => {
                settings.producers = parse_usize_arg(&args, index, "--producers");
                index += 2;
            }
            "--orders" | "--attempts" => {
                settings.orders_per_producer = parse_usize_arg(&args, index, "--orders");
                index += 2;
            }
            "--queue-capacity" => {
                settings.queue_capacity = parse_usize_arg(&args, index, "--queue-capacity");
                index += 2;
            }
            "--analytics-records" => {
                settings.analytics_records = parse_usize_arg(&args, index, "--analytics-records");
                index += 2;
            }
            "--service-delay-us" => {
                settings.service_delay_micros = parse_usize_arg(&args, index, "--service-delay-us");
                index += 2;
            }
            "--help" | "-h" => print_usage_and_exit(),
            unknown => {
                eprintln!("Unknown argument: {unknown}");
                print_usage_and_exit();
            }
        }
    }

    if settings.tickets == 0
        || settings.producers == 0
        || settings.orders_per_producer == 0
        || settings.analytics_records == 0
    {
        eprintln!("--tickets, --producers, --orders, and --analytics-records must be positive.");
        std::process::exit(2);
    }

    (mode, settings)
}

fn parse_usize_arg(args: &[String], index: usize, name: &str) -> usize {
    let Some(value) = args.get(index + 1) else {
        eprintln!("Missing value for {name}");
        print_usage_and_exit();
    };

    value.parse::<usize>().unwrap_or_else(|_| {
        eprintln!("{name} must be a non-negative integer, got {value:?}");
        std::process::exit(2);
    })
}

fn print_usage_and_exit() -> ! {
    eprintln!(
        "Usage:
  cargo run --release -- [all|mpsc|rayon] [options]

Options:
  --tickets N             Initial tickets in the live sale service
  --producers N           Concurrent front-end/order producer threads
  --orders N              Orders submitted by each producer
  --queue-capacity N      Bounded mpsc queue capacity; 0 means rendezvous
  --service-delay-us N    Simulated central ticket-office processing delay
  --analytics-records N   Batch records used by the Rayon analytics demo

Examples:
  cargo run --release -- mpsc --tickets 100 --producers 16 --orders 80 --queue-capacity 32
  cargo run --release -- rayon --analytics-records 1000000 --producers 16"
    );
    std::process::exit(2);
}

fn print_banner(settings: Settings) {
    println!("Concert Ticket Platform Concurrency Demo");
    println!(
        "tickets={}, producers={}, orders_per_producer={}, total_orders={}, queue_capacity={}, analytics_records={}",
        settings.tickets,
        settings.producers,
        settings.orders_per_producer,
        settings.total_orders(),
        settings.queue_capacity,
        settings.analytics_records
    );
    println!();
}

fn create_inventory(ticket_count: usize) -> Inventory {
    let vip_count = ticket_count / 5;
    let regular_count = ticket_count - vip_count;

    let regular = (1..=regular_count)
        .map(|id| Ticket {
            id,
            tier: TicketTier::Regular,
            price_cents: TicketTier::Regular.price_cents(),
        })
        .collect();

    let vip = (1..=vip_count)
        .map(|offset| {
            let id = regular_count + offset;
            Ticket {
                id,
                tier: TicketTier::Vip,
                price_cents: TicketTier::Vip.price_cents(),
            }
        })
        .collect();

    Inventory { regular, vip }
}

fn make_order_id(source_id: usize, sequence: usize, orders_per_producer: usize) -> usize {
    source_id * orders_per_producer + sequence + 1
}

fn pick_requested_tier(source_id: usize, sequence: usize) -> TicketTier {
    if (source_id * 13 + sequence * 7) % 5 == 0 {
        TicketTier::Vip
    } else {
        TicketTier::Regular
    }
}

fn run_mpsc_order_service(settings: Settings) -> OrderServiceReport {
    let started = Instant::now();
    let (order_tx, order_rx) = mpsc::sync_channel::<Order>(settings.queue_capacity);
    let mut handles = Vec::with_capacity(settings.producers);

    for source_id in 0..settings.producers {
        let order_tx = order_tx.clone();
        handles.push(thread::spawn(move || {
            produce_orders(source_id, settings.orders_per_producer, order_tx)
        }));
    }

    drop(order_tx);

    let mut inventory = create_inventory(settings.tickets);
    let mut sales = Vec::with_capacity(settings.tickets);
    let mut rejected_sold_out = 0usize;
    let service_delay = Duration::from_micros(settings.service_delay_micros as u64);

    for order in order_rx {
        if !service_delay.is_zero() {
            thread::sleep(service_delay);
        }

        if let Some(ticket) = inventory.pop_for(order.requested_tier) {
            sales.push(Sale {
                order_id: order.id,
                customer_id: order.customer_id,
                source_id: order.source_id,
                ticket_id: ticket.id,
                tier: ticket.tier,
                price_cents: ticket.price_cents,
            });
        } else {
            rejected_sold_out += 1;
        }
    }

    let mut producer_summaries = Vec::with_capacity(settings.producers);
    for handle in handles {
        producer_summaries.push(handle.join().expect("order producer thread panicked"));
    }
    producer_summaries.sort_by_key(|summary| summary.source_id);

    let regular_sold = sales
        .iter()
        .filter(|sale| sale.tier == TicketTier::Regular)
        .count();
    let vip_sold = sales.len() - regular_sold;

    OrderServiceReport {
        total_inventory: settings.tickets,
        total_orders: settings.total_orders(),
        sold: sales.len(),
        rejected_sold_out,
        regular_sold,
        vip_sold,
        regular_remaining: inventory.regular.len(),
        vip_remaining: inventory.vip.len(),
        queue_capacity: settings.queue_capacity,
        service_delay_micros: settings.service_delay_micros,
        elapsed: started.elapsed(),
        producer_summaries,
        sales,
    }
}

fn produce_orders(
    source_id: usize,
    orders_per_producer: usize,
    order_tx: mpsc::SyncSender<Order>,
) -> ProducerSummary {
    let mut blocked_sends = 0usize;
    let mut send_wait_total = Duration::ZERO;
    let blocked_threshold = Duration::from_micros(100);

    for sequence in 0..orders_per_producer {
        thread::yield_now();
        let order_id = make_order_id(source_id, sequence, orders_per_producer);
        let order = Order {
            id: order_id,
            customer_id: 10_000 + order_id,
            source_id,
            requested_tier: pick_requested_tier(source_id, sequence),
        };

        let send_started = Instant::now();
        order_tx
            .send(order)
            .expect("central ticket office receiver closed unexpectedly");
        let send_wait = send_started.elapsed();

        if send_wait >= blocked_threshold {
            blocked_sends += 1;
        }
        send_wait_total += send_wait;
    }

    ProducerSummary {
        source_id,
        submitted: orders_per_producer,
        blocked_sends,
        send_wait_total,
    }
}

fn run_analytics_benchmark(settings: Settings, seed_sales: &[Sale]) -> AnalyticsBenchmark {
    let dataset =
        build_analytics_dataset(seed_sales, settings.analytics_records, settings.producers);

    let sequential_started = Instant::now();
    let sequential = analyze_sales_sequential(&dataset, settings.producers);
    let sequential_elapsed = sequential_started.elapsed();

    let parallel_started = Instant::now();
    let parallel = analyze_sales_parallel(&dataset, settings.producers);
    let parallel_elapsed = parallel_started.elapsed();

    debug_assert_eq!(sequential, parallel);

    AnalyticsBenchmark {
        sequential,
        parallel,
        sequential_elapsed,
        parallel_elapsed,
    }
}

fn build_analytics_dataset(
    seed_sales: &[Sale],
    target_records: usize,
    sources: usize,
) -> Vec<Sale> {
    let target_records = target_records.max(seed_sales.len());
    let mut sales = Vec::with_capacity(target_records);

    sales.extend_from_slice(seed_sales);

    while sales.len() < target_records {
        let index = sales.len();
        let source_id = index % sources;
        let tier = if (index * 11 + source_id * 3) % 7 == 0 {
            TicketTier::Vip
        } else {
            TicketTier::Regular
        };
        let surge_cents = ((index % 9) as u64) * 25;
        let price_cents = tier.price_cents() + surge_cents;

        sales.push(Sale {
            order_id: index + 1,
            customer_id: 50_000 + index,
            source_id,
            ticket_id: index + 1,
            tier,
            price_cents,
        });
    }

    sales
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AnalyticsAccumulator {
    records: usize,
    revenue_cents: u64,
    regular_sales: usize,
    vip_sales: usize,
    high_value_sales: usize,
    review_required: usize,
    per_source_sales: Vec<usize>,
    checksum: u64,
}

impl AnalyticsAccumulator {
    fn new(sources: usize) -> Self {
        Self {
            records: 0,
            revenue_cents: 0,
            regular_sales: 0,
            vip_sales: 0,
            high_value_sales: 0,
            review_required: 0,
            per_source_sales: vec![0; sources],
            checksum: 0,
        }
    }

    fn record_sale(&mut self, sale: &Sale) {
        self.records += 1;
        self.revenue_cents += sale.price_cents;
        self.per_source_sales[sale.source_id] += 1;

        match sale.tier {
            TicketTier::Regular => self.regular_sales += 1,
            TicketTier::Vip => self.vip_sales += 1,
        }

        if sale.price_cents >= 3_800 {
            self.high_value_sales += 1;
        }

        let risk_score = calculate_risk_score(sale);
        if risk_score >= 985 || (sale.tier == TicketTier::Vip && risk_score >= 950) {
            self.review_required += 1;
        }

        self.checksum ^= sale_checksum(sale, risk_score);
    }

    fn merge(mut self, other: Self) -> Self {
        self.records += other.records;
        self.revenue_cents += other.revenue_cents;
        self.regular_sales += other.regular_sales;
        self.vip_sales += other.vip_sales;
        self.high_value_sales += other.high_value_sales;
        self.review_required += other.review_required;
        self.checksum ^= other.checksum;

        for (index, count) in other.per_source_sales.into_iter().enumerate() {
            self.per_source_sales[index] += count;
        }

        self
    }

    fn finalize(self) -> AnalyticsResult {
        let (busiest_source, busiest_source_sales) = self
            .per_source_sales
            .iter()
            .copied()
            .enumerate()
            .max_by_key(|(_, count)| *count)
            .unwrap_or((0, 0));

        AnalyticsResult {
            records: self.records,
            revenue_cents: self.revenue_cents,
            regular_sales: self.regular_sales,
            vip_sales: self.vip_sales,
            high_value_sales: self.high_value_sales,
            review_required: self.review_required,
            busiest_source,
            busiest_source_sales,
            per_source_sales: self.per_source_sales,
            checksum: self.checksum,
        }
    }
}

fn analyze_sales_sequential(sales: &[Sale], sources: usize) -> AnalyticsResult {
    let mut accumulator = AnalyticsAccumulator::new(sources);
    for sale in sales {
        accumulator.record_sale(sale);
    }
    accumulator.finalize()
}

fn analyze_sales_parallel(sales: &[Sale], sources: usize) -> AnalyticsResult {
    sales
        .par_iter()
        .fold(
            || AnalyticsAccumulator::new(sources),
            |mut accumulator, sale| {
                accumulator.record_sale(sale);
                accumulator
            },
        )
        .reduce(
            || AnalyticsAccumulator::new(sources),
            |left, right| left.merge(right),
        )
        .finalize()
}

fn calculate_risk_score(sale: &Sale) -> u32 {
    let mut value = (sale.order_id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (sale.customer_id as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
        ^ (sale.ticket_id as u64).wrapping_mul(0x94D0_49BB_1331_11EB)
        ^ sale.price_cents;

    for round in 0..32 {
        value ^= value >> 30;
        value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value ^= value >> 27;
        value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^= value >> 31;
        value ^= round;
    }

    (value % 1000) as u32
}

fn sale_checksum(sale: &Sale, risk_score: u32) -> u64 {
    (sale.order_id as u64 * 31)
        ^ (sale.customer_id as u64 * 17)
        ^ (sale.source_id as u64 * 13)
        ^ (sale.ticket_id as u64 * 7)
        ^ sale.price_cents
        ^ risk_score as u64
}

fn print_order_service_report(report: &OrderServiceReport) {
    println!("Stage 2: Rust mpsc bounded order service");
    println!("  model: many front-end producers -> bounded channel -> one ticket office");
    println!("  queue capacity: {}", report.queue_capacity);
    println!(
        "  service delay per order: {} us",
        report.service_delay_micros
    );
    println!("  submitted orders: {}", report.total_orders);
    println!(
        "  producer submitted count: {}",
        report.submitted_by_producers()
    );
    println!("  sold: {}", report.sold);
    println!("  rejected/sold out: {}", report.rejected_sold_out);
    println!(
        "  sold by tier: regular={}, vip={}",
        report.regular_sold, report.vip_sold
    );
    println!(
        "  remaining by tier: regular={}, vip={}",
        report.regular_remaining, report.vip_remaining
    );
    println!(
        "  invariant sold + remaining == initial and sold + rejected == submitted: {}",
        report.invariant_ok()
    );
    println!(
        "  producer sends delayed by backpressure: {}",
        report.blocked_sends()
    );
    println!(
        "  cumulative producer send wait: {:?}",
        report.total_send_wait()
    );
    println!("  elapsed: {:?}", report.elapsed);
}

fn print_analytics_benchmark(benchmark: &AnalyticsBenchmark) {
    println!("Stage 3: Rayon batch analytics");
    println!("  model: independent sales records -> map/fold/reduce across CPU cores");
    println!("  sequential elapsed: {:?}", benchmark.sequential_elapsed);
    println!("  parallel elapsed: {:?}", benchmark.parallel_elapsed);
    println!(
        "  speedup: {:.2}x",
        speedup(benchmark.sequential_elapsed, benchmark.parallel_elapsed)
    );
    println!(
        "  sequential result equals parallel result: {}",
        benchmark.sequential == benchmark.parallel
    );
    print!("{}", benchmark.parallel);
    println!("  note: Rayon accelerates independent batch analytics; mpsc models live task flow.");
}

fn speedup(sequential: Duration, parallel: Duration) -> f64 {
    let parallel_secs = parallel.as_secs_f64();
    if parallel_secs == 0.0 {
        return 0.0;
    }
    sequential.as_secs_f64() / parallel_secs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_settings() -> Settings {
        Settings {
            tickets: 25,
            producers: 8,
            orders_per_producer: 20,
            queue_capacity: 4,
            analytics_records: 10_000,
            service_delay_micros: 0,
        }
    }

    #[test]
    fn mpsc_order_service_preserves_business_invariants() {
        let report = run_mpsc_order_service(fast_settings());

        assert!(report.invariant_ok());
        assert_eq!(report.sold, report.sales.len());
        assert!(report.sold <= report.total_inventory);
    }

    #[test]
    fn bounded_queue_receives_every_submitted_order() {
        let report = run_mpsc_order_service(fast_settings());
        let submitted_by_producers: usize = report
            .producer_summaries
            .iter()
            .map(|summary| summary.submitted)
            .sum();

        assert_eq!(submitted_by_producers, report.total_orders);
        assert_eq!(report.sold + report.rejected_sold_out, report.total_orders);
    }

    #[test]
    fn rayon_parallel_analytics_matches_sequential_analytics() {
        let settings = fast_settings();
        let live_report = run_mpsc_order_service(settings);
        let benchmark = run_analytics_benchmark(settings, &live_report.sales);

        assert_eq!(benchmark.sequential, benchmark.parallel);
        assert_eq!(benchmark.parallel.records, settings.analytics_records);
        assert_eq!(
            benchmark.parallel.per_source_sales.iter().sum::<usize>(),
            benchmark.parallel.records
        );
    }
}
