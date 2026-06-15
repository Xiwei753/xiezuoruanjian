import { describe, it, expect } from '@ohos/hypium'
import { WriterCoreBridge } from '../../../main/ets/bridge/WriterCoreBridge'

export default function writerCoreBridgeTest() {
  describe('WriterCoreBridge', () => {
    it('should return mock instance when native is unavailable', () => {
      const bridge = WriterCoreBridge.getInstance()
      expect(bridge).not.toBeNull()
    })
  })
}