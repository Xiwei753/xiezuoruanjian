// ⚠️ UNVERIFIED — 本接口未经 DevEco 编译验证。
// 方法签名基于 HarmonyOS NEXT 文档推测，必须用 DevEco 真编译校验。

export interface FileEntry {
  name: string
  path: string
  isDirectory: boolean
  size: number
  lastModified: number
}

export interface WorkspaceFileAccess {
  // Workspace operations
  initializeWorkspace(path: string): Promise<boolean>
  validateWorkspace(path: string): Promise<WorkspaceValidationResult>

  // Directory operations
  listDirectory(path: string): Promise<FileEntry[]>
  createDirectory(path: string): Promise<boolean>
  deleteDirectory(path: string): Promise<boolean>
  renameDirectory(oldPath: string, newPath: string): Promise<boolean>

  // File operations
  readFile(path: string): Promise<string>
  writeFile(path: string, content: string): Promise<boolean>
  deleteFile(path: string): Promise<boolean>
  renameFile(oldPath: string, newPath: string): Promise<boolean>
  fileExists(path: string): Promise<boolean>

  // Project structure (legacy/mock-only — will be removed when Core owns all path logic)
  /** @legacy Mock-only. Production path resolution must go through Core. */
  getProjectPath(projectId: string): string
  /** @legacy Mock-only. Production path resolution must go through Core. */
  getVolumePath(projectId: string, volumeId: string): string
  /** @legacy Mock-only. Production path resolution must go through Core. */
  getChapterPath(projectId: string, volumeId: string, chapterId: string): string
  /** @legacy Mock-only. Production path resolution must go through Core. */
  getSettingsPath(): string
  /** @legacy Mock-only. Production path resolution must go through Core. */
  getStarMapPath(starmapId: string): string

  // Metadata
  readJsonFile<T>(path: string): Promise<T | null>
  writeJsonFile<T>(path: string, data: T): Promise<boolean>
}

export interface WorkspaceValidationResult {
  isValid: boolean
  errors: string[]
  warnings: string[]
}

export class MockFileAccess implements WorkspaceFileAccess {
  private basePath: string = '/mock/workspace'

  async initializeWorkspace(path: string): Promise<boolean> {
    this.basePath = path
    return true
  }

  async validateWorkspace(path: string): Promise<WorkspaceValidationResult> {
    return {
      isValid: true,
      errors: [],
      warnings: []
    }
  }

  async listDirectory(path: string): Promise<FileEntry[]> {
    return []
  }

  async createDirectory(path: string): Promise<boolean> {
    return true
  }

  async deleteDirectory(path: string): Promise<boolean> {
    return true
  }

  async renameDirectory(oldPath: string, newPath: string): Promise<boolean> {
    return true
  }

  async readFile(path: string): Promise<string> {
    return ''
  }

  async writeFile(path: string, content: string): Promise<boolean> {
    return true
  }

  async deleteFile(path: string): Promise<boolean> {
    return true
  }

  async renameFile(oldPath: string, newPath: string): Promise<boolean> {
    return true
  }

  async fileExists(path: string): Promise<boolean> {
    return true
  }

  /** @legacy Mock-only. Production path resolution must go through Core. */
  getProjectPath(projectId: string): string {
    return `${this.basePath}/projects/${projectId}`
  }

  /** @legacy Mock-only. Production path resolution must go through Core. */
  getVolumePath(projectId: string, volumeId: string): string {
    return `${this.getProjectPath(projectId)}/volumes/${volumeId}`
  }

  /** @legacy Mock-only. Production path resolution must go through Core. */
  getChapterPath(projectId: string, volumeId: string, chapterId: string): string {
    return `${this.getVolumePath(projectId, volumeId)}/chapters/${chapterId}.md`
  }

  /** @legacy Mock-only. Production path resolution must go through Core. */
  getSettingsPath(): string {
    return `${this.basePath}/settings.json`
  }

  /** @legacy Mock-only. Production path resolution must go through Core. */
  getStarMapPath(starmapId: string): string {
    return `${this.basePath}/starmaps/${starmapId}`
  }

  async readJsonFile<T>(path: string): Promise<T | null> {
    return null
  }

  async writeJsonFile<T>(path: string, data: T): Promise<boolean> {
    return true
  }
}

export class NativeFileAccess implements WorkspaceFileAccess {
  // TODO: Implement with real file system access via NAPI/C-ABI
  // This will connect to Rust Core's file operations

  async initializeWorkspace(path: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }

  async validateWorkspace(path: string): Promise<WorkspaceValidationResult> {
    throw new Error('NativeFileAccess not implemented')
  }

  async listDirectory(path: string): Promise<FileEntry[]> {
    throw new Error('NativeFileAccess not implemented')
  }

  async createDirectory(path: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }

  async deleteDirectory(path: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }

  async renameDirectory(oldPath: string, newPath: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }

  async readFile(path: string): Promise<string> {
    throw new Error('NativeFileAccess not implemented')
  }

  async writeFile(path: string, content: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }

  async deleteFile(path: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }

  async renameFile(oldPath: string, newPath: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }

  async fileExists(path: string): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }

  getProjectPath(projectId: string): string {
    throw new Error('NativeFileAccess not implemented')
  }

  getVolumePath(projectId: string, volumeId: string): string {
    throw new Error('NativeFileAccess not implemented')
  }

  getChapterPath(projectId: string, volumeId: string, chapterId: string): string {
    throw new Error('NativeFileAccess not implemented')
  }

  getSettingsPath(): string {
    throw new Error('NativeFileAccess not implemented')
  }

  getStarMapPath(starmapId: string): string {
    throw new Error('NativeFileAccess not implemented')
  }

  async readJsonFile<T>(path: string): Promise<T | null> {
    throw new Error('NativeFileAccess not implemented')
  }

  async writeJsonFile<T>(path: string, data: T): Promise<boolean> {
    throw new Error('NativeFileAccess not implemented')
  }
}
