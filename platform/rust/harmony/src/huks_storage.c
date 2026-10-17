/*
 * 素笺 HarmonyOS HUKS 安全存储 C shim（Issue #924）。
 *
 * 职责：
 * - 用 HUKS NDK 托管一个设备级不可导出 AES-256 密钥（alias 固定）；
 * - 提供 AES-GCM 单次会话加解密（密文输出为 `ciphertext || tag`，tag 16 字节）；
 * - nonce 从平台随机源 /dev/urandom 读取，读失败即报错，不做降级；
 * - 首次使用时生成密钥（幂等），密钥存在性经 OH_Huks_IsKeyItemExist 判定。
 *
 * 本文件只依赖 HUKS NDK 的公开 C 接口（libhuks_ndk.z.so），
 * 类型与内存生命周期全部在 shim 内消化，Rust 侧只看到字节缓冲接口。
 */

#include "huks_storage.h"

#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include "native_huks_api.h"
#include "native_huks_param.h"
#include "native_huks_type.h"

/* 设备级密钥别名：随存储格式升级时递增后缀，旧别名不删除。 */
static const char *const SUJIAN_HUKS_KEY_ALIAS = "sujian_secure_storage_v1";

/* HUKS 单次调用允许的参数个数上限：算法/用途/密钥长度/填充/GCM/nonce/tag。 */
#define SUJIAN_HUKS_MAX_PARAMS 8u

/* shim 本地错误码（与 HUKS 正错误码区分）。 */
#define SUJIAN_HUKS_LOCAL_ERROR_INVALID  (-1)
#define SUJIAN_HUKS_LOCAL_ERROR_RANDOM   (-2)
#define SUJIAN_HUKS_LOCAL_ERROR_SELFTEST (-3)

static void set_error(char *error_out, uint32_t error_out_len, const char *message)
{
    if (error_out == NULL || error_out_len == 0) {
        return;
    }
    (void)snprintf(error_out, (size_t)error_out_len, "%s", message);
}

static void set_huks_error(char *error_out, uint32_t error_out_len, const char *stage,
                           OH_Huks_Result result)
{
    if (error_out == NULL || error_out_len == 0) {
        return;
    }
    (void)snprintf(error_out, (size_t)error_out_len, "%s failed: HUKS error %d (%s)", stage,
                   result.errorCode, result.errorMsg == NULL ? "no message" : result.errorMsg);
}

/* 从 /dev/urandom 读取随机字节；失败返回 -2，不降级为弱随机。 */
static int32_t fill_random(uint8_t *buffer, uint32_t length, char *error_out, uint32_t error_out_len)
{
    int fd = open("/dev/urandom", O_RDONLY | O_CLOEXEC);
    if (fd < 0) {
        set_error(error_out, error_out_len, "open /dev/urandom failed");
        return SUJIAN_HUKS_LOCAL_ERROR_RANDOM;
    }
    uint32_t filled = 0;
    while (filled < length) {
        ssize_t read_size = read(fd, buffer + filled, (size_t)(length - filled));
        if (read_size <= 0) {
            (void)close(fd);
            set_error(error_out, error_out_len, "read /dev/urandom failed");
            return SUJIAN_HUKS_LOCAL_ERROR_RANDOM;
        }
        filled += (uint32_t)read_size;
    }
    (void)close(fd);
    return 0;
}

/*
 * 构造 AES-256 / GCM 参数集：ALGORITHM + PURPOSE + KEY_SIZE + PADDING + BLOCK_MODE，
 * 可选 NONCE（加密/解密）与 AE_TAG（解密）。成功时返回已构建的参数集，由调用方
 * 用 OH_Huks_FreeParamSet 释放。
 */
static OH_Huks_Result build_param_set(OH_Huks_ParamSet **param_set, uint32_t purpose,
                                      const uint8_t *nonce, uint32_t nonce_len,
                                      const uint8_t *ae_tag, uint32_t ae_tag_len)
{
    OH_Huks_Result result = OH_Huks_InitParamSet(param_set);
    if (result.errorCode != OH_HUKS_SUCCESS) {
        return result;
    }

    OH_Huks_Param params[SUJIAN_HUKS_MAX_PARAMS];
    (void)memset(params, 0, sizeof(params));
    uint32_t count = 0;

    params[count].tag = OH_HUKS_TAG_ALGORITHM;
    params[count].uint32Param = OH_HUKS_ALG_AES;
    count++;
    params[count].tag = OH_HUKS_TAG_PURPOSE;
    params[count].uint32Param = purpose;
    count++;
    params[count].tag = OH_HUKS_TAG_KEY_SIZE;
    params[count].uint32Param = OH_HUKS_AES_KEY_SIZE_256;
    count++;
    params[count].tag = OH_HUKS_TAG_PADDING;
    params[count].uint32Param = OH_HUKS_PADDING_NONE;
    count++;
    params[count].tag = OH_HUKS_TAG_BLOCK_MODE;
    params[count].uint32Param = OH_HUKS_MODE_GCM;
    count++;

    if (nonce != NULL && nonce_len > 0) {
        /* HUKS 只读 NONCE；指向调用方缓冲，BuildParamSet 会拷贝。 */
        params[count].tag = OH_HUKS_TAG_NONCE;
        params[count].blob.size = nonce_len;
        params[count].blob.data = (uint8_t *)(uintptr_t)nonce;
        count++;
    }
    if (ae_tag != NULL && ae_tag_len > 0) {
        params[count].tag = OH_HUKS_TAG_AE_TAG;
        params[count].blob.size = ae_tag_len;
        params[count].blob.data = (uint8_t *)(uintptr_t)ae_tag;
        count++;
    }

    result = OH_Huks_AddParams(*param_set, params, count);
    if (result.errorCode != OH_HUKS_SUCCESS) {
        OH_Huks_FreeParamSet(param_set);
        return result;
    }
    result = OH_Huks_BuildParamSet(param_set);
    if (result.errorCode != OH_HUKS_SUCCESS) {
        OH_Huks_FreeParamSet(param_set);
    }
    return result;
}

/* 确保设备级密钥存在（不存在则生成）；已存在时不重建。 */
static int32_t ensure_key(char *error_out, uint32_t error_out_len)
{
    OH_Huks_Blob alias = { (uint32_t)strlen(SUJIAN_HUKS_KEY_ALIAS),
                           (uint8_t *)(uintptr_t)SUJIAN_HUKS_KEY_ALIAS };

    OH_Huks_ParamSet *check_set = NULL;
    OH_Huks_Result result =
        build_param_set(&check_set, OH_HUKS_KEY_PURPOSE_ENCRYPT, NULL, 0, NULL, 0);
    if (result.errorCode != OH_HUKS_SUCCESS) {
        set_huks_error(error_out, error_out_len, "build key check params", result);
        return result.errorCode;
    }
    result = OH_Huks_IsKeyItemExist(&alias, check_set);
    OH_Huks_FreeParamSet(&check_set);
    if (result.errorCode == OH_HUKS_SUCCESS) {
        return 0;
    }
    if (result.errorCode != OH_HUKS_ERR_CODE_ITEM_NOT_EXIST) {
        set_huks_error(error_out, error_out_len, "check key existence", result);
        return result.errorCode;
    }

    OH_Huks_ParamSet *gen_set = NULL;
    result = build_param_set(&gen_set, OH_HUKS_KEY_PURPOSE_ENCRYPT | OH_HUKS_KEY_PURPOSE_DECRYPT,
                             NULL, 0, NULL, 0);
    if (result.errorCode != OH_HUKS_SUCCESS) {
        set_huks_error(error_out, error_out_len, "build key generation params", result);
        return result.errorCode;
    }
    result = OH_Huks_GenerateKeyItem(&alias, gen_set, NULL);
    OH_Huks_FreeParamSet(&gen_set);
    if (result.errorCode == OH_HUKS_SUCCESS ||
        result.errorCode == OH_HUKS_ERR_CODE_KEY_ALREADY_EXIST) {
        return 0;
    }
    set_huks_error(error_out, error_out_len, "generate key", result);
    return result.errorCode;
}

/*
 * 单次 GCM 会话：InitSession + FinishSession。
 * in/out 缓冲语义由调用方保证容量，成功后 *output_len 写实际输出长度。
 */
static int32_t run_cipher_op(uint32_t purpose, const uint8_t *nonce, uint32_t nonce_len,
                             const uint8_t *ae_tag, uint32_t ae_tag_len, const uint8_t *input,
                             uint32_t input_len, uint8_t *output, uint32_t *output_len,
                             char *error_out, uint32_t error_out_len)
{
    int32_t key_error = ensure_key(error_out, error_out_len);
    if (key_error != 0) {
        return key_error;
    }

    OH_Huks_Blob alias = { (uint32_t)strlen(SUJIAN_HUKS_KEY_ALIAS),
                           (uint8_t *)(uintptr_t)SUJIAN_HUKS_KEY_ALIAS };

    OH_Huks_ParamSet *params = NULL;
    OH_Huks_Result result = build_param_set(&params, purpose, nonce, nonce_len, ae_tag, ae_tag_len);
    if (result.errorCode != OH_HUKS_SUCCESS) {
        set_huks_error(error_out, error_out_len, "build session params", result);
        return result.errorCode;
    }

    uint8_t handle_data[8] = { 0 };
    OH_Huks_Blob handle = { (uint32_t)sizeof(handle_data), handle_data };
    result = OH_Huks_InitSession(&alias, params, &handle, NULL);
    if (result.errorCode != OH_HUKS_SUCCESS) {
        OH_Huks_FreeParamSet(&params);
        set_huks_error(error_out, error_out_len, "init session", result);
        return result.errorCode;
    }

    /* 空输入时给一个有效指针，HUKS 对 0 长度数据仍要求非空指针。 */
    static uint8_t empty_input = 0;
    OH_Huks_Blob in_blob = { input_len, (uint8_t *)(input != NULL ? input : &empty_input) };
    OH_Huks_Blob out_blob = { *output_len, output };

    result = OH_Huks_FinishSession(&handle, params, &in_blob, &out_blob);
    OH_Huks_FreeParamSet(&params);
    if (result.errorCode != OH_HUKS_SUCCESS) {
        set_huks_error(error_out, error_out_len, "finish session", result);
        return result.errorCode;
    }

    *output_len = out_blob.size;
    return 0;
}

int32_t sujian_huks_probe(char *error_out, uint32_t error_out_len)
{
    int32_t result = ensure_key(error_out, error_out_len);
    if (result != 0) {
        return result;
    }

    /* 回环自检：验证密钥可用、AES-GCM 输出布局（ciphertext || tag）与解密路径一致。 */
    static const uint8_t sample[] = "sujian-huks-selftest";
    uint8_t nonce[SUJIAN_HUKS_NONCE_LEN] = { 0 };
    uint8_t ciphertext[sizeof(sample) + SUJIAN_HUKS_TAG_LEN] = { 0 };
    uint32_t ciphertext_len = (uint32_t)sizeof(ciphertext);

    result = sujian_huks_encrypt(sample, (uint32_t)sizeof(sample), nonce,
                                 (uint32_t)sizeof(nonce), ciphertext, &ciphertext_len, error_out,
                                 error_out_len);
    if (result != 0) {
        return result;
    }

    uint8_t plaintext[sizeof(sample)] = { 0 };
    uint32_t plaintext_len = (uint32_t)sizeof(plaintext);
    result = sujian_huks_decrypt(nonce, (uint32_t)sizeof(nonce), ciphertext, ciphertext_len,
                                 plaintext, &plaintext_len, error_out, error_out_len);
    if (result != 0) {
        return result;
    }
    if (plaintext_len != (uint32_t)sizeof(sample) ||
        memcmp(plaintext, sample, sizeof(sample)) != 0) {
        set_error(error_out, error_out_len, "HUKS self-test roundtrip mismatch");
        return SUJIAN_HUKS_LOCAL_ERROR_SELFTEST;
    }
    return 0;
}

int32_t sujian_huks_encrypt(const uint8_t *plaintext, uint32_t plaintext_len,
                            uint8_t *nonce_out, uint32_t nonce_cap, uint8_t *ciphertext_out,
                            uint32_t *ciphertext_len, char *error_out, uint32_t error_out_len)
{
    if (nonce_out == NULL || nonce_cap < SUJIAN_HUKS_NONCE_LEN || ciphertext_out == NULL ||
        ciphertext_len == NULL || (plaintext == NULL && plaintext_len > 0) ||
        *ciphertext_len < plaintext_len + SUJIAN_HUKS_TAG_LEN) {
        set_error(error_out, error_out_len, "invalid argument or ciphertext buffer too small");
        return SUJIAN_HUKS_LOCAL_ERROR_INVALID;
    }

    int32_t result = fill_random(nonce_out, SUJIAN_HUKS_NONCE_LEN, error_out, error_out_len);
    if (result != 0) {
        return result;
    }

    return run_cipher_op(OH_HUKS_KEY_PURPOSE_ENCRYPT, nonce_out, SUJIAN_HUKS_NONCE_LEN, NULL, 0,
                         plaintext, plaintext_len, ciphertext_out, ciphertext_len, error_out,
                         error_out_len);
}

int32_t sujian_huks_decrypt(const uint8_t *nonce, uint32_t nonce_len, const uint8_t *ciphertext,
                            uint32_t ciphertext_len, uint8_t *plaintext_out,
                            uint32_t *plaintext_len, char *error_out, uint32_t error_out_len)
{
    if (nonce == NULL || nonce_len == 0 || ciphertext == NULL ||
        ciphertext_len < SUJIAN_HUKS_TAG_LEN || plaintext_out == NULL || plaintext_len == NULL ||
        *plaintext_len < ciphertext_len - SUJIAN_HUKS_TAG_LEN) {
        set_error(error_out, error_out_len, "invalid argument or plaintext buffer too small");
        return SUJIAN_HUKS_LOCAL_ERROR_INVALID;
    }

    const uint8_t *ae_tag = ciphertext + (ciphertext_len - SUJIAN_HUKS_TAG_LEN);
    uint32_t body_len = ciphertext_len - SUJIAN_HUKS_TAG_LEN;

    return run_cipher_op(OH_HUKS_KEY_PURPOSE_DECRYPT, nonce, nonce_len, ae_tag,
                         SUJIAN_HUKS_TAG_LEN, ciphertext, body_len, plaintext_out, plaintext_len,
                         error_out, error_out_len);
}
