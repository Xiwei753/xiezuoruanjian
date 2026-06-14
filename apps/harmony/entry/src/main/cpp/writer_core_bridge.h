#ifndef WRITER_CORE_BRIDGE_H
#define WRITER_CORE_BRIDGE_H

#include <cstdint>

#ifdef __cplusplus
extern "C" {
#endif

int32_t writer_core_init(const char* path);
char* writer_core_get_load_status(void);
int32_t writer_core_calculate_word_count(const char* text);
void writer_core_free_string(char* ptr);

#ifdef __cplusplus
}
#endif

#endif
