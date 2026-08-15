import { describe, it, expect } from '@ohos/hypium'
import { NativeWriterCoreBridge } from '../../../main/ets/corebridge/NativeWriterCoreBridge'

export default function writerCoreBridgeTest() {
  describe('WriterCoreBridge', () => {
    it('should return native instance', () => {
      const bridge = NativeWriterCoreBridge.getInstance()
      expect(bridge).not.toBeNull()
    })
  })
}
