import { describe, it, expect } from '@ohos/hypium'
import { CoreDtos } from '../../../main/ets/model/CoreDtos'

export default function coreDtosTest() {
  describe('CoreDtos', () => {
    it('ResultEnvelope success should have data field', () => {
      const envelope: CoreDtos.ResultEnvelope<string> = {
        success: true,
        data: 'test'
      }
      expect(envelope.success).assertTrue()
      expect(envelope.data).assertEqual('test')
    })

    it('ResultEnvelope failure should have errorCode field', () => {
      const envelope: CoreDtos.ResultEnvelope<string> = {
        success: false,
        errorCode: 'TEST_ERROR'
      }
      expect(envelope.success).assertFalse()
      expect(envelope.errorCode).assertEqual('TEST_ERROR')
    })
  })
}