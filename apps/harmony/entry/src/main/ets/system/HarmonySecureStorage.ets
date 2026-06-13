export interface TokenStore {
  // Token operations
  getToken(key: string): Promise<string | null>
  setToken(key: string, token: string): Promise<boolean>
  deleteToken(key: string): Promise<boolean>
  hasToken(key: string): Promise<boolean>

  // Sync credentials
  getSyncToken(): Promise<string | null>
  setSyncToken(token: string): Promise<boolean>
  clearSyncToken(): Promise<boolean>

  // GitHub token
  getGitHubToken(): Promise<string | null>
  setGitHubToken(token: string): Promise<boolean>
  clearGitHubToken(): Promise<boolean>

  // API keys
  getApiKey(service: string): Promise<string | null>
  setApiKey(service: string, key: string): Promise<boolean>
  clearApiKey(service: string): Promise<boolean>

  // Secure data
  getSecureData(key: string): Promise<string | null>
  setSecureData(key: string, data: string): Promise<boolean>
  deleteSecureData(key: string): Promise<boolean>

  // Bulk operations
  clearAll(): Promise<boolean>
  listKeys(): Promise<string[]>
}

export interface EncryptionProvider {
  encrypt(data: string): Promise<string>
  decrypt(encryptedData: string): Promise<string>
  generateKey(): Promise<string>
  isAvailable(): Promise<boolean>
}

export class MockTokenStore implements TokenStore {
  private storage: Map<string, string> = new Map()

  async getToken(key: string): Promise<string | null> {
    return this.storage.get(key) || null
  }

  async setToken(key: string, token: string): Promise<boolean> {
    this.storage.set(key, token)
    return true
  }

  async deleteToken(key: string): Promise<boolean> {
    this.storage.delete(key)
    return true
  }

  async hasToken(key: string): Promise<boolean> {
    return this.storage.has(key)
  }

  async getSyncToken(): Promise<string | null> {
    return this.getToken('sync_token')
  }

  async setSyncToken(token: string): Promise<boolean> {
    return this.setToken('sync_token', token)
  }

  async clearSyncToken(): Promise<boolean> {
    return this.deleteToken('sync_token')
  }

  async getGitHubToken(): Promise<string | null> {
    return this.getToken('github_token')
  }

  async setGitHubToken(token: string): Promise<boolean> {
    return this.setToken('github_token', token)
  }

  async clearGitHubToken(): Promise<boolean> {
    return this.deleteToken('github_token')
  }

  async getApiKey(service: string): Promise<string | null> {
    return this.getToken(`api_key_${service}`)
  }

  async setApiKey(service: string, key: string): Promise<boolean> {
    return this.setToken(`api_key_${service}`, key)
  }

  async clearApiKey(service: string): Promise<boolean> {
    return this.deleteToken(`api_key_${service}`)
  }

  async getSecureData(key: string): Promise<string | null> {
    return this.getToken(key)
  }

  async setSecureData(key: string, data: string): Promise<boolean> {
    return this.setToken(key, data)
  }

  async deleteSecureData(key: string): Promise<boolean> {
    return this.deleteToken(key)
  }

  async clearAll(): Promise<boolean> {
    this.storage.clear()
    return true
  }

  async listKeys(): Promise<string[]> {
    return Array.from(this.storage.keys())
  }
}

export class NativeTokenStore implements TokenStore {
  // TODO: Implement with HarmonyOS KeyStore API
  // This will use @ohos.security.keyStore for secure storage

  async getToken(key: string): Promise<string | null> {
    throw new Error('NativeTokenStore not implemented')
  }

  async setToken(key: string, token: string): Promise<boolean> {
    throw new Error('NativeTokenStore not implemented')
  }

  async deleteToken(key: string): Promise<boolean> {
    throw new Error('NativeTokenStore not implemented')
  }

  async hasToken(key: string): Promise<boolean> {
    throw new Error('NativeTokenStore not implemented')
  }

  async getSyncToken(): Promise<string | null> {
    throw new Error('NativeTokenStore not implemented')
  }

  async setSyncToken(token: string): Promise<boolean> {
    throw new Error('NativeTokenStore not implemented')
  }

  async clearSyncToken(): Promise<boolean> {
    throw new Error('NativeTokenStore not implemented')
  }

  async getGitHubToken(): Promise<string | null> {
    throw new Error('NativeTokenStore not implemented')
  }

  async setGitHubToken(token: string): Promise<boolean> {
    throw new Error('NativeTokenStore not implemented')
  }

  async clearGitHubToken(): Promise<boolean> {
    throw new Error('NativeTokenStore not implemented')
  }

  async getApiKey(service: string): Promise<string | null> {
    throw new Error('NativeTokenStore not implemented')
  }

  async setApiKey(service: string, key: string): Promise<boolean> {
    throw new Error('NativeTokenStore not implemented')
  }

  async clearApiKey(service: string): Promise<boolean> {
    throw new Error('NativeTokenStore not implemented')
  }

  async getSecureData(key: string): Promise<string | null> {
    throw new Error('NativeTokenStore not implemented')
  }

  async setSecureData(key: string, data: string): Promise<boolean> {
    throw new Error('NativeTokenStore not implemented')
  }

  async deleteSecureData(key: string): Promise<boolean> {
    throw new Error('NativeTokenStore not implemented')
  }

  async clearAll(): Promise<boolean> {
    throw new Error('NativeTokenStore not implemented')
  }

  async listKeys(): Promise<string[]> {
    throw new Error('NativeTokenStore not implemented')
  }
}

export class MockEncryptionProvider implements EncryptionProvider {
  async encrypt(data: string): Promise<string> {
    // Simple base64 encoding for mock
    return btoa(data)
  }

  async decrypt(encryptedData: string): Promise<string> {
    // Simple base64 decoding for mock
    return atob(encryptedData)
  }

  async generateKey(): Promise<string> {
    return 'mock-encryption-key-' + Date.now()
  }

  async isAvailable(): Promise<boolean> {
    return true
  }
}
