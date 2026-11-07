#ifndef LITHOVM_CANDIDATE_V1_H
#define LITHOVM_CANDIDATE_V1_H
#include <stddef.h>
#include <stdint.h>
typedef struct { uint8_t *data; size_t len; } LithoBuffer;
/* -1 absent, -2 failure; capacity=0 asks for exact length. Never retain pointers. */
typedef intptr_t (*LithoRead)(uintptr_t, const uint8_t *, size_t, uint8_t *, size_t);
/* Return 1 when fuel was debited; 0 on exhaustion/failure. */
typedef int32_t (*LithoCharge)(uintptr_t, uint64_t);
int32_t lithovm_execute_v1(const uint8_t *, size_t, uintptr_t, LithoRead, LithoBuffer *);
int32_t lithovm_execute_v2(const uint8_t *, size_t, uintptr_t, LithoRead, LithoCharge, LithoBuffer *);
/* Disabled custody candidate only. ReadBalance(context,address20,output32)
 * returns 1 on a metered SDK balance read, otherwise failure. The reader must
 * validate bank/account policy and must not retain either pointer. Request
 * version 2 and live fuel are required; returned balance changes remain staged.
 * v1/v2 retain the existing nonpayable profile and request version 1. */
typedef int32_t (*LithoReadBalance)(uintptr_t, const uint8_t *, uint8_t *);
int32_t lithovm_execute_v3(const uint8_t *, size_t, uintptr_t, LithoRead, LithoReadBalance, LithoCharge, LithoBuffer *);
void lithovm_free_v1(LithoBuffer);
#endif
