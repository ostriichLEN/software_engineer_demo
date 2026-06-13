#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
#else
#include <time.h>
#endif

typedef enum {
    TIER_REGULAR = 0,
    TIER_VIP = 1
} TicketTier;

typedef struct {
    int id;
    int attempts;
    int local_sold;
    int local_failed;
} WorkerArgs;

typedef struct {
    int id;
    int source_id;
    TicketTier requested_tier;
} Order;

typedef struct {
    Order *items;
    int capacity;
    int head;
    int tail;
    int count;
    int closed;
    pthread_mutex_t mutex;
    pthread_cond_t not_empty;
    pthread_cond_t not_full;
} OrderQueue;

typedef struct {
    int source_id;
    int attempts;
    OrderQueue *queue;
    int blocked_sends;
    double send_wait_total_seconds;
} ProducerArgs;

typedef struct {
    int initial_tickets;
    int regular_left;
    int vip_left;
    int service_delay_us;
    OrderQueue *queue;
    int sold;
    int rejected_sold_out;
    int regular_sold;
    int vip_sold;
} TicketOfficeArgs;

static int initial_tickets = 100;
static int tickets_left = 100;
static pthread_mutex_t ticket_lock = PTHREAD_MUTEX_INITIALIZER;

static double now_seconds(void) {
#ifdef _WIN32
    LARGE_INTEGER frequency;
    LARGE_INTEGER counter;
    QueryPerformanceFrequency(&frequency);
    QueryPerformanceCounter(&counter);
    return (double)counter.QuadPart / (double)frequency.QuadPart;
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec + (double)ts.tv_nsec / 1000000000.0;
#endif
}

static void sleep_micros(int micros) {
    if (micros <= 0) {
        return;
    }

    double end = now_seconds() + (double)micros / 1000000.0;
    while (now_seconds() < end) {
        /* Busy wait keeps the demo delay close to microseconds on Windows. */
    }
}

static void tiny_delay(int seed) {
    volatile int waste = 0;
    for (int i = 0; i < 400 + (seed % 200); i++) {
        waste += i;
    }
}

static void widen_race_window(void) {
    sleep_micros(1000);
}

static TicketTier pick_requested_tier(int source_id, int sequence) {
    return ((source_id * 13 + sequence * 7) % 5 == 0) ? TIER_VIP : TIER_REGULAR;
}

static int parse_positive(const char *raw, const char *name) {
    char *end = NULL;
    long value = strtol(raw, &end, 10);
    if (*raw == '\0' || *end != '\0' || value <= 0 || value > 1000000) {
        fprintf(stderr, "%s must be a positive integer, got '%s'\n", name, raw);
        exit(2);
    }
    return (int)value;
}

static int parse_non_negative(const char *raw, const char *name) {
    char *end = NULL;
    long value = strtol(raw, &end, 10);
    if (*raw == '\0' || *end != '\0' || value < 0 || value > 1000000) {
        fprintf(stderr, "%s must be a non-negative integer, got '%s'\n", name, raw);
        exit(2);
    }
    return (int)value;
}

static void *unsafe_worker(void *raw_args) {
    WorkerArgs *args = (WorkerArgs *)raw_args;
    int sold = 0;
    int failed = 0;

    for (int attempt = 0; attempt < args->attempts; attempt++) {
        int observed = tickets_left;
        if (observed > 0) {
            tiny_delay(args->id + attempt);
            widen_race_window();
            tickets_left = observed - 1;
            sold++;
        } else {
            failed++;
        }
    }

    args->local_sold = sold;
    args->local_failed = failed;
    return NULL;
}

static void *mutex_worker(void *raw_args) {
    WorkerArgs *args = (WorkerArgs *)raw_args;
    int sold = 0;
    int failed = 0;

    for (int attempt = 0; attempt < args->attempts; attempt++) {
        pthread_mutex_lock(&ticket_lock);
        if (tickets_left > 0) {
            tiny_delay(args->id + attempt);
            tickets_left--;
            sold++;
        } else {
            failed++;
        }
        pthread_mutex_unlock(&ticket_lock);
    }

    args->local_sold = sold;
    args->local_failed = failed;
    return NULL;
}

static void queue_init(OrderQueue *queue, int capacity) {
    queue->items = calloc((size_t)capacity, sizeof(Order));
    if (queue->items == NULL) {
        fprintf(stderr, "queue allocation failed\n");
        exit(1);
    }

    queue->capacity = capacity;
    queue->head = 0;
    queue->tail = 0;
    queue->count = 0;
    queue->closed = 0;
    pthread_mutex_init(&queue->mutex, NULL);
    pthread_cond_init(&queue->not_empty, NULL);
    pthread_cond_init(&queue->not_full, NULL);
}

static void queue_destroy(OrderQueue *queue) {
    free(queue->items);
    pthread_mutex_destroy(&queue->mutex);
    pthread_cond_destroy(&queue->not_empty);
    pthread_cond_destroy(&queue->not_full);
}

static void queue_close(OrderQueue *queue) {
    pthread_mutex_lock(&queue->mutex);
    queue->closed = 1;
    pthread_cond_broadcast(&queue->not_empty);
    pthread_cond_broadcast(&queue->not_full);
    pthread_mutex_unlock(&queue->mutex);
}

static int queue_push(OrderQueue *queue, Order order, int *was_blocked) {
    pthread_mutex_lock(&queue->mutex);

    *was_blocked = 0;
    while (queue->count == queue->capacity && !queue->closed) {
        *was_blocked = 1;
        pthread_cond_wait(&queue->not_full, &queue->mutex);
    }

    if (queue->closed) {
        pthread_mutex_unlock(&queue->mutex);
        return 0;
    }

    queue->items[queue->tail] = order;
    queue->tail = (queue->tail + 1) % queue->capacity;
    queue->count++;
    pthread_cond_signal(&queue->not_empty);

    pthread_mutex_unlock(&queue->mutex);
    return 1;
}

static int queue_pop(OrderQueue *queue, Order *order) {
    pthread_mutex_lock(&queue->mutex);

    while (queue->count == 0 && !queue->closed) {
        pthread_cond_wait(&queue->not_empty, &queue->mutex);
    }

    if (queue->count == 0 && queue->closed) {
        pthread_mutex_unlock(&queue->mutex);
        return 0;
    }

    *order = queue->items[queue->head];
    queue->head = (queue->head + 1) % queue->capacity;
    queue->count--;
    pthread_cond_signal(&queue->not_full);

    pthread_mutex_unlock(&queue->mutex);
    return 1;
}

static void *producer_worker(void *raw_args) {
    ProducerArgs *args = (ProducerArgs *)raw_args;

    for (int attempt = 0; attempt < args->attempts; attempt++) {
        Order order;
        order.id = args->source_id * args->attempts + attempt + 1;
        order.source_id = args->source_id;
        order.requested_tier = pick_requested_tier(args->source_id, attempt);

        int was_blocked = 0;
        double started = now_seconds();
        if (!queue_push(args->queue, order, &was_blocked)) {
            break;
        }
        double elapsed = now_seconds() - started;

        if (was_blocked) {
            args->blocked_sends++;
        }
        args->send_wait_total_seconds += elapsed;
    }

    return NULL;
}

static void *ticket_office_worker(void *raw_args) {
    TicketOfficeArgs *args = (TicketOfficeArgs *)raw_args;
    Order order;

    while (queue_pop(args->queue, &order)) {
        sleep_micros(args->service_delay_us);

        if (order.requested_tier == TIER_VIP) {
            if (args->vip_left > 0) {
                args->vip_left--;
                args->vip_sold++;
                args->sold++;
            } else {
                args->rejected_sold_out++;
            }
        } else {
            if (args->regular_left > 0) {
                args->regular_left--;
                args->regular_sold++;
                args->sold++;
            } else {
                args->rejected_sold_out++;
            }
        }
    }

    return NULL;
}

static int run_shared_counter_mode(const char *mode, int worker_count, int attempts) {
    tickets_left = initial_tickets;

    pthread_t *threads = calloc((size_t)worker_count, sizeof(pthread_t));
    WorkerArgs *args = calloc((size_t)worker_count, sizeof(WorkerArgs));
    if (threads == NULL || args == NULL) {
        fprintf(stderr, "allocation failed\n");
        free(threads);
        free(args);
        return 1;
    }

    for (int i = 0; i < worker_count; i++) {
        args[i].id = i;
        args[i].attempts = attempts;
        void *(*worker)(void *) = strcmp(mode, "mutex") == 0 ? mutex_worker : unsafe_worker;
        int rc = pthread_create(&threads[i], NULL, worker, &args[i]);
        if (rc != 0) {
            fprintf(stderr, "pthread_create failed at worker %d\n", i);
            free(threads);
            free(args);
            return 1;
        }
    }

    int sold = 0;
    int failed = 0;
    for (int i = 0; i < worker_count; i++) {
        pthread_join(threads[i], NULL);
        sold += args[i].local_sold;
        failed += args[i].local_failed;
    }

    printf("C ticket demo (%s shared counter)\n", mode);
    printf("  tickets=%d, threads=%d, attempts_per_thread=%d, total_requests=%d\n",
           initial_tickets, worker_count, attempts, worker_count * attempts);
    printf("  sold=%d\n", sold);
    printf("  failed/no ticket=%d\n", failed);
    printf("  remaining=%d\n", tickets_left);
    printf("  invariant sold + remaining == initial: %s\n",
           (sold + tickets_left == initial_tickets) ? "true" : "false");
    printf("  oversold: %s\n", sold > initial_tickets ? "true" : "false");

    free(threads);
    free(args);
    return 0;
}

static int run_queue_mode(int worker_count, int attempts, int queue_capacity, int service_delay_us) {
    OrderQueue queue;
    queue_init(&queue, queue_capacity);

    pthread_t office_thread;
    TicketOfficeArgs office_args;
    int vip_tickets = initial_tickets / 5;
    int regular_tickets = initial_tickets - vip_tickets;

    office_args.initial_tickets = initial_tickets;
    office_args.regular_left = regular_tickets;
    office_args.vip_left = vip_tickets;
    office_args.service_delay_us = service_delay_us;
    office_args.queue = &queue;
    office_args.sold = 0;
    office_args.rejected_sold_out = 0;
    office_args.regular_sold = 0;
    office_args.vip_sold = 0;

    double started = now_seconds();
    int rc = pthread_create(&office_thread, NULL, ticket_office_worker, &office_args);
    if (rc != 0) {
        fprintf(stderr, "pthread_create failed for ticket office\n");
        queue_destroy(&queue);
        return 1;
    }

    pthread_t *producer_threads = calloc((size_t)worker_count, sizeof(pthread_t));
    ProducerArgs *producer_args = calloc((size_t)worker_count, sizeof(ProducerArgs));
    if (producer_threads == NULL || producer_args == NULL) {
        fprintf(stderr, "allocation failed\n");
        free(producer_threads);
        free(producer_args);
        queue_close(&queue);
        pthread_join(office_thread, NULL);
        queue_destroy(&queue);
        return 1;
    }

    for (int i = 0; i < worker_count; i++) {
        producer_args[i].source_id = i;
        producer_args[i].attempts = attempts;
        producer_args[i].queue = &queue;
        producer_args[i].blocked_sends = 0;
        producer_args[i].send_wait_total_seconds = 0.0;

        rc = pthread_create(&producer_threads[i], NULL, producer_worker, &producer_args[i]);
        if (rc != 0) {
            fprintf(stderr, "pthread_create failed at producer %d\n", i);
            queue_close(&queue);
            free(producer_threads);
            free(producer_args);
            pthread_join(office_thread, NULL);
            queue_destroy(&queue);
            return 1;
        }
    }

    int submitted = 0;
    int blocked_sends = 0;
    double cumulative_send_wait = 0.0;
    for (int i = 0; i < worker_count; i++) {
        pthread_join(producer_threads[i], NULL);
        submitted += producer_args[i].attempts;
        blocked_sends += producer_args[i].blocked_sends;
        cumulative_send_wait += producer_args[i].send_wait_total_seconds;
    }

    queue_close(&queue);
    pthread_join(office_thread, NULL);
    double elapsed = now_seconds() - started;

    int remaining = office_args.regular_left + office_args.vip_left;
    int total_requests = worker_count * attempts;
    int invariant_ok = (office_args.sold + remaining == initial_tickets) &&
                       (office_args.sold + office_args.rejected_sold_out == submitted);

    printf("C ticket demo (queue single-owner ticket office)\n");
    printf("  model: producer threads -> bounded pthread queue -> one ticket office\n");
    printf("  tickets=%d, producers=%d, orders_per_producer=%d, total_orders=%d\n",
           initial_tickets, worker_count, attempts, total_requests);
    printf("  queue capacity: %d\n", queue_capacity);
    printf("  service delay per order: %d us\n", service_delay_us);
    printf("  submitted orders: %d\n", submitted);
    printf("  sold: %d\n", office_args.sold);
    printf("  rejected/sold out: %d\n", office_args.rejected_sold_out);
    printf("  sold by tier: regular=%d, vip=%d\n", office_args.regular_sold, office_args.vip_sold);
    printf("  remaining by tier: regular=%d, vip=%d\n", office_args.regular_left, office_args.vip_left);
    printf("  invariant sold + remaining == initial and sold + rejected == submitted: %s\n",
           invariant_ok ? "true" : "false");
    printf("  producer sends delayed by backpressure: %d\n", blocked_sends);
    printf("  cumulative producer send wait: %.6fs\n", cumulative_send_wait);
    printf("  elapsed: %.6fs\n", elapsed);

    free(producer_threads);
    free(producer_args);
    queue_destroy(&queue);
    return 0;
}

int main(int argc, char **argv) {
    const char *mode = argc > 1 ? argv[1] : "unsafe";
    initial_tickets = argc > 2 ? parse_positive(argv[2], "tickets") : 100;
    int worker_count = argc > 3 ? parse_positive(argv[3], "threads/producers") : 16;
    int attempts = argc > 4 ? parse_positive(argv[4], "attempts/orders") : 80;
    int queue_capacity = argc > 5 ? parse_positive(argv[5], "queue_capacity") : 32;
    int service_delay_us = argc > 6 ? parse_non_negative(argv[6], "service_delay_us") : 500;

    if (strcmp(mode, "unsafe") == 0 || strcmp(mode, "mutex") == 0) {
        return run_shared_counter_mode(mode, worker_count, attempts);
    }

    if (strcmp(mode, "queue") == 0) {
        return run_queue_mode(worker_count, attempts, queue_capacity, service_delay_us);
    }

    fprintf(stderr,
            "Usage: %s [unsafe|mutex|queue] [tickets] [threads/producers] [attempts/orders] [queue_capacity] [service_delay_us]\n",
            argv[0]);
    return 2;
}
