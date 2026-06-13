#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <omp.h>

typedef enum {
    TIER_REGULAR = 0,
    TIER_VIP = 1
} TicketTier;

typedef struct {
    size_t order_id;
    size_t customer_id;
    size_t source_id;
    size_t ticket_id;
    TicketTier tier;
    uint64_t price_cents;
} Sale;

typedef struct {
    size_t records;
    uint64_t revenue_cents;
    size_t regular_sales;
    size_t vip_sales;
    size_t high_value_sales;
    size_t review_required;
    size_t busiest_source;
    size_t busiest_source_sales;
    uint64_t *per_source_sales;
    uint64_t checksum;
} AnalyticsResult;

static size_t parse_size(const char *raw, const char *name) {
    char *end = NULL;
    unsigned long long value = strtoull(raw, &end, 10);
    if (*raw == '\0' || *end != '\0' || value == 0) {
        fprintf(stderr, "%s must be a positive integer, got '%s'\n", name, raw);
        exit(2);
    }
    return (size_t)value;
}

static uint64_t ticket_price(TicketTier tier) {
    return tier == TIER_VIP ? 3800ULL : 1200ULL;
}

static void build_dataset(Sale *sales, size_t records, size_t sources) {
    for (size_t index = 0; index < records; index++) {
        size_t source_id = index % sources;
        TicketTier tier = ((index * 11 + source_id * 3) % 7 == 0) ? TIER_VIP : TIER_REGULAR;
        uint64_t surge_cents = (uint64_t)(index % 9) * 25ULL;

        sales[index].order_id = index + 1;
        sales[index].customer_id = 50000 + index;
        sales[index].source_id = source_id;
        sales[index].ticket_id = index + 1;
        sales[index].tier = tier;
        sales[index].price_cents = ticket_price(tier) + surge_cents;
    }
}

static uint32_t calculate_risk_score(const Sale *sale) {
    uint64_t value = ((uint64_t)sale->order_id * 0x9E3779B97F4A7C15ULL) ^
                     ((uint64_t)sale->customer_id * 0xBF58476D1CE4E5B9ULL) ^
                     ((uint64_t)sale->ticket_id * 0x94D049BB133111EBULL) ^
                     sale->price_cents;

    for (uint64_t round = 0; round < 32; round++) {
        value ^= value >> 30;
        value *= 0xBF58476D1CE4E5B9ULL;
        value ^= value >> 27;
        value *= 0x94D049BB133111EBULL;
        value ^= value >> 31;
        value ^= round;
    }

    return (uint32_t)(value % 1000ULL);
}

static uint64_t sale_checksum(const Sale *sale, uint32_t risk_score) {
    return ((uint64_t)sale->order_id * 31ULL) ^
           ((uint64_t)sale->customer_id * 17ULL) ^
           ((uint64_t)sale->source_id * 13ULL) ^
           ((uint64_t)sale->ticket_id * 7ULL) ^
           sale->price_cents ^
           (uint64_t)risk_score;
}

static AnalyticsResult result_new(size_t sources) {
    AnalyticsResult result;
    result.records = 0;
    result.revenue_cents = 0;
    result.regular_sales = 0;
    result.vip_sales = 0;
    result.high_value_sales = 0;
    result.review_required = 0;
    result.busiest_source = 0;
    result.busiest_source_sales = 0;
    result.per_source_sales = calloc(sources, sizeof(uint64_t));
    result.checksum = 0;

    if (result.per_source_sales == NULL) {
        fprintf(stderr, "result allocation failed\n");
        exit(1);
    }

    return result;
}

static void result_finalize(AnalyticsResult *result, size_t sources) {
    for (size_t source = 0; source < sources; source++) {
        if (result->per_source_sales[source] >= result->busiest_source_sales) {
            result->busiest_source = source;
            result->busiest_source_sales = (size_t)result->per_source_sales[source];
        }
    }
}

static void result_free(AnalyticsResult *result) {
    free(result->per_source_sales);
    result->per_source_sales = NULL;
}

static AnalyticsResult analyze_sequential(const Sale *sales, size_t records, size_t sources) {
    AnalyticsResult result = result_new(sources);

    for (size_t index = 0; index < records; index++) {
        const Sale *sale = &sales[index];
        uint32_t risk_score = calculate_risk_score(sale);

        result.records++;
        result.revenue_cents += sale->price_cents;
        result.per_source_sales[sale->source_id]++;

        if (sale->tier == TIER_VIP) {
            result.vip_sales++;
        } else {
            result.regular_sales++;
        }

        if (sale->price_cents >= 3800ULL) {
            result.high_value_sales++;
        }

        if (risk_score >= 985 || (sale->tier == TIER_VIP && risk_score >= 950)) {
            result.review_required++;
        }

        result.checksum ^= sale_checksum(sale, risk_score);
    }

    result_finalize(&result, sources);
    return result;
}

static AnalyticsResult analyze_openmp(const Sale *sales, size_t records, size_t sources) {
    AnalyticsResult result = result_new(sources);
    int max_threads = omp_get_max_threads();
    uint64_t *thread_counts = calloc((size_t)max_threads * sources, sizeof(uint64_t));

    if (thread_counts == NULL) {
        fprintf(stderr, "thread count allocation failed\n");
        exit(1);
    }

    uint64_t revenue_cents = 0;
    uint64_t regular_sales = 0;
    uint64_t vip_sales = 0;
    uint64_t high_value_sales = 0;
    uint64_t review_required = 0;
    uint64_t checksum = 0;

#pragma omp parallel reduction(+ : revenue_cents, regular_sales, vip_sales, high_value_sales, review_required) reduction(^ : checksum)
    {
        int thread_id = omp_get_thread_num();
        uint64_t *local_counts = &thread_counts[(size_t)thread_id * sources];

#pragma omp for schedule(static)
        for (size_t index = 0; index < records; index++) {
            const Sale *sale = &sales[index];
            uint32_t risk_score = calculate_risk_score(sale);

            revenue_cents += sale->price_cents;
            local_counts[sale->source_id]++;

            if (sale->tier == TIER_VIP) {
                vip_sales++;
            } else {
                regular_sales++;
            }

            if (sale->price_cents >= 3800ULL) {
                high_value_sales++;
            }

            if (risk_score >= 985 || (sale->tier == TIER_VIP && risk_score >= 950)) {
                review_required++;
            }

            checksum ^= sale_checksum(sale, risk_score);
        }
    }

    result.records = records;
    result.revenue_cents = revenue_cents;
    result.regular_sales = (size_t)regular_sales;
    result.vip_sales = (size_t)vip_sales;
    result.high_value_sales = (size_t)high_value_sales;
    result.review_required = (size_t)review_required;
    result.checksum = checksum;

    for (int thread_id = 0; thread_id < max_threads; thread_id++) {
        uint64_t *local_counts = &thread_counts[(size_t)thread_id * sources];
        for (size_t source = 0; source < sources; source++) {
            result.per_source_sales[source] += local_counts[source];
        }
    }

    result_finalize(&result, sources);
    free(thread_counts);
    return result;
}

static int results_equal(const AnalyticsResult *left, const AnalyticsResult *right, size_t sources) {
    if (left->records != right->records ||
        left->revenue_cents != right->revenue_cents ||
        left->regular_sales != right->regular_sales ||
        left->vip_sales != right->vip_sales ||
        left->high_value_sales != right->high_value_sales ||
        left->review_required != right->review_required ||
        left->busiest_source != right->busiest_source ||
        left->busiest_source_sales != right->busiest_source_sales ||
        left->checksum != right->checksum) {
        return 0;
    }

    for (size_t source = 0; source < sources; source++) {
        if (left->per_source_sales[source] != right->per_source_sales[source]) {
            return 0;
        }
    }

    return 1;
}

static void print_result(const AnalyticsResult *result) {
    printf("  records: %zu\n", result->records);
    printf("  revenue: $%.2f\n", (double)result->revenue_cents / 100.0);
    printf("  ticket tiers: regular=%zu, vip=%zu\n", result->regular_sales, result->vip_sales);
    printf("  high value sales: %zu\n", result->high_value_sales);
    printf("  fraud/manual review candidates: %zu\n", result->review_required);
    printf("  busiest source: #%zu (%zu sales)\n", result->busiest_source, result->busiest_source_sales);
    printf("  checksum: %llu\n", (unsigned long long)result->checksum);
}

int main(int argc, char **argv) {
    size_t records = argc > 1 ? parse_size(argv[1], "records") : 1000000;
    size_t sources = argc > 2 ? parse_size(argv[2], "sources") : 16;

    Sale *sales = malloc(records * sizeof(Sale));
    if (sales == NULL) {
        fprintf(stderr, "sales allocation failed\n");
        return 1;
    }

    double build_started = omp_get_wtime();
    build_dataset(sales, records, sources);
    double build_elapsed_ms = (omp_get_wtime() - build_started) * 1000.0;

    double sequential_started = omp_get_wtime();
    AnalyticsResult sequential = analyze_sequential(sales, records, sources);
    double sequential_elapsed_ms = (omp_get_wtime() - sequential_started) * 1000.0;

    double parallel_started = omp_get_wtime();
    AnalyticsResult parallel = analyze_openmp(sales, records, sources);
    double parallel_elapsed_ms = (omp_get_wtime() - parallel_started) * 1000.0;

    int equal = results_equal(&sequential, &parallel, sources);
    double speedup = parallel_elapsed_ms == 0.0 ? 0.0 : sequential_elapsed_ms / parallel_elapsed_ms;

    printf("C OpenMP batch analytics\n");
    printf("  records=%zu, sources=%zu, max_threads=%d\n", records, sources, omp_get_max_threads());
    printf("  dataset build elapsed: %.4fms\n", build_elapsed_ms);
    printf("  sequential elapsed: %.4fms\n", sequential_elapsed_ms);
    printf("  openmp elapsed: %.4fms\n", parallel_elapsed_ms);
    printf("  speedup: %.2fx\n", speedup);
    printf("  sequential result equals openmp result: %s\n", equal ? "true" : "false");
    print_result(&parallel);

    result_free(&sequential);
    result_free(&parallel);
    free(sales);

    return equal ? 0 : 1;
}

