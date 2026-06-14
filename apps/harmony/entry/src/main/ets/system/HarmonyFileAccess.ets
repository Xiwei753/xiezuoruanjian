// HarmonyOS NEXT File Access — 包装 @ohos.file.fs（API 12）。
// NativeFileAccess 内部应 import fs from '@ohos.file.fs'。
// 接口签名对齐 fs.open() / fs.read() / fs.write() / fs.stat() / fs.mkdir() 等。

export interface FileEntry {
  name: string
  isDirectory: boolean
  size: number
}

export interface WorkspaceFileAccess {
  initializeWorkspace(path: string): Promise<boolean>
  validateWorkspace(path: string): Promise<WorkspaceValidationResult>

  listDirectory(path: string): Promise<FileEntry[]>
  createDirectory(path: string): Promise<boolean>
  deleteDirectory(path: string): Promise<boolean>
  renameDirectory(oldPath: string, newPath: string): Promise<boolean>

  readFile(path: string): Promise<string>
  writeFile(path: string, content: string): Promise<void>
  deleteFile(path: string): Promise<boolean>
  renameFile(oldPath: string, newPath: string): Promise<boolean>
  fileExists(path: string): Promise<boolean>

  readJsonFile<T>(path: string): Promise<T | null>
  writeJsonFile<T>(path: string, data: T): Promise<void>
}

export interface WorkspaceValidationResult {
  isValid: boolean
  errors: string[]
}

export class MockFileAccess implements WorkspaceFileAccess {
  private basePath: string = '/mock/workspace'

  async initializeWorkspace(path: string): Promise<boolean> {
    this.basePath = path
    return true
  }

  async validateWorkspace(_path: string): Promise<WorkspaceValidationResult> {
    return { isValid: true, errors: [] }
  }

  async listDirectory(_path: string): Promise<FileEntry[]> {
    return []
  }

  async createDirectory(_path: string): Promise<boolean> {
    return true
  }

  async deleteDirectory(_path: string): Promise<boolean> {
    return true
  }

  async renameDirectory(_oldPath: string, _newPath: string): Promise<boolean> {
    return true
  }

  async readFile(_path: string): Promise<string> {
    return ''
  }

  async writeFile(_path: string, _content: string): Promise<void> {
  }

  async deleteFile(_path: string): Promise<boolean> {
    return true
  }

  async renameFile(_oldPath: string, _newPath: string): Promise<boolean> {
    return true
  }

  async fileExists(_path: string): Promise<boolean> {
    return true
  }

  async readJsonFile<T>(_path: string): Promise<T | null> {
    return null
  }

  async writeJsonFile<T>(_path: string, _data: T): Promise<void> {
  }
}

export class NativeFileAccess implements WorkspaceFileAccess {
  // TODO: 用 @ohos.file.fs 实现。
  // import fs from '@ohos.file.fs'
  // fs.open(path, fs.OpenMode.READ_WRITE) / fs.read() / fs.write() / fs.close()
  // fs.stat(path) / fs.mkdir(path) / fs.unlink(path) / fs.rename(old, new)
  // fs.listFile(path) 返回 string[]

  async initializeWorkspace(_path: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented — 需对接 @ohos.file.fs')
  }

  async validateWorkspace(_path: string): Promise<WorkspaceValidationResult> {
    throw new Error('NativeFileAccess not implemented')
  }

  async listDirectory(_path: string): Promise<FileEntry[]> {
    throw new Error('NativeFileAccess not implemented')
  }

  async createDirectory(_path: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }

  async deleteDirectory(_path: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }

  async renameDirectory(_oldPath: string, _newPath: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }

  async readFile(_path: string): Promise<string> {
    throw new Error('NativeFileAccess not implemented')
  }

  async writeFile(_path: string, _content: string): Promise<void> {
    throw new Error('NativeFileAccess not implemented')
  }

  async deleteFile(_path: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }

  async renameFile(_oldPath: string, _newPath: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }

  async fileExists(_path: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }

  async readJsonFile<T>(_path: string): Promise<T | null> {
    throw new Error('NativeFileAccess not implemented')
  }

  async writeJsonFile<T>(_path: string, _data: T): Promise<void> {
    throw new Error('NativeFileAccess not implemented')
  }
}
