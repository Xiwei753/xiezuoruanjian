// GripPostureTypes — 素笺稳定的握持姿态状态。
// 不依赖任何 @kit.*，可被 Service 和各 apiXX impl 共享。
// 系统枚举（motion.HoldingHandStatus）到稳定状态的映射在各 impl/apiXX facade 内完成。
export type GripPosture = 'unknown' | 'notHeld' | 'left' | 'right' | 'both'

export interface GripPostureObserver {
  onGripPostureChanged?(posture: GripPosture): void
}
