#ifndef RANDOMX_WRAPPER_H
#define RANDOMX_WRAPPER_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

// RandomX API structures (opaque types)
typedef struct randomx_cache randomx_cache;
typedef struct randomx_dataset randomx_dataset;
typedef struct randomx_vm randomx_vm;

// RandomX flags
#define RANDOMX_FLAG_DEFAULT        0
#define RANDOMX_FLAG_LARGE_PAGES    1
#define RANDOMX_FLAG_HARD_AES       2
#define RANDOMX_FLAG_FULL_MEM       4
#define RANDOMX_FLAG_JIT            8
#define RANDOMX_FLAG_SECURE         16
#define RANDOMX_FLAG_ARGON2_SSSE3   32
#define RANDOMX_FLAG_ARGON2_AVX2    64
#define RANDOMX_FLAG_ARGON2         96

// Hash size constant
#define RANDOMX_HASH_SIZE           32

// Cache management
randomx_cache* randomx_alloc_cache(int flags);
void randomx_init_cache(randomx_cache* cache, const void* key, size_t keySize);
void randomx_release_cache(randomx_cache* cache);

// Dataset management
randomx_dataset* randomx_alloc_dataset(int flags);
unsigned long randomx_dataset_item_count(void);
void randomx_init_dataset(randomx_dataset* dataset, randomx_cache* cache, unsigned long startItem, unsigned long itemCount);
void randomx_release_dataset(randomx_dataset* dataset);

// VM management
randomx_vm* randomx_create_vm(int flags, randomx_cache* cache, randomx_dataset* dataset);
void randomx_vm_set_cache(randomx_vm* machine, randomx_cache* cache);
void randomx_vm_set_dataset(randomx_vm* machine, randomx_dataset* dataset);
void randomx_destroy_vm(randomx_vm* machine);

// Hash calculation
void randomx_calculate_hash(randomx_vm* machine, const void* input, size_t inputSize, void* output);
void randomx_calculate_hash_first(randomx_vm* machine, const void* input, size_t inputSize);
void randomx_calculate_hash_next(randomx_vm* machine, const void* nextInput, size_t nextInputSize, void* output);

// Utility functions
int randomx_get_flags(void);

#ifdef __cplusplus
}
#endif

#endif // RANDOMX_WRAPPER_H