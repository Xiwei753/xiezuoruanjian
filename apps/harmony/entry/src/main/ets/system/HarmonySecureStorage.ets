// HarmonyOS NEXT Storage — 包装 @ohos.data.preferences（API 12）。
// NativeTokenStore 内部应 import dataPreferences from '@ohos.data.preferences'。
// 接口签名对齐 preferences.get() / preferences.put() / preferences.flush()。

export interface KVStore {
  get(key: string): Promise<string | null>
  put(key: string, value: string): Promise<void>
  has(key: string): Promise<boolean>
  delete(key: string): Promise<void>
  clear(): Promise<void>
  keys(): Promise<string[]>
}

export class MockKVStore implements KVStore {
  private storage: Map<string, string> = new Map()

  async get(key: string): Promise<string | null> {
    return this.storage.get(key) ?? null
  }

  async put(key: string, value: string): Promise<void> {
    this.storage.set(key, value)
  }

  async has(key: string): Promise<boolean> {
    return this.storage.has(key)
  }

  async delete(key: string): Promise<void> {
    this.storage.delete(key)
  }

  async clear(): Promise<void> {
    this.storage.clear()
  }

  async keys(): Promise<string[]> {
    return Array.from(this.storage.keys())
  }
}

export class NativeKVStore implements KVStore {
  // TODO: 用 @ohos.data.preferences 实现。
  // import dataPreferences from '@ohos.data.preferences'
  // const prefs = await dataPreferences.getPreferences(context, 'writer_kv')
  // await prefs.get(key) / prefs.put(key, value) / prefs.flush()

  async get(_key: string): Promise<string | null> {
    throw new Error('NativeKVStore not implemented — 需对接 @ohos.data.preferences')
  }

  async put(_key: string, _value: string): Promise<void> {
    throw new Error('NativeKVStore not implemented')
  }

  async has(_key: string): Promise<boolean> {
    throw new Error('NativeKVStore not implemented')
  }

  async delete(_key: string): Promise<void> {
    throw new Error('NativeKVStore not implemented')
  }

  async clear(): Promise<void> {
    throw new Error('NativeKVStore not implemented')
  }

  async keys(): Promise<string[]> {
    throw new Error('NativeKVStore not implemented')
  }
}
