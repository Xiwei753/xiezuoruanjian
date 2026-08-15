import { describe, it, expect } from '@ohos/hypium'
import { ResultEnvelope } from '../../../main/ets/corebridge/dto/ResultEnvelope'

export default function coreDtosTest() {
  describe('CoreDtos', () => {
    it('ResultEnvelope success should have data field', () => {
      const envelope: ResultEnvelope<string> = {
        success: true,
        data: 'test',
        warnings: [],
        changedPaths: [],
        changedEntities: []
      }
      expect(envelope.success).assertTrue()
      expect(envelope.data).assertEqual('test')
    })

    it('ResultEnvelope failure should have errorCode field', () => {
      const envelope: ResultEnvelope<string> = {
        success: false,
        errorCode: 'TEST_ERROR',
        warnings: [],
        changedPaths: [],
        changedEntities: []
      }
      expect(envelope.success).assertFalse()
      expect(envelope.errorCode).assertEqual('TEST_ERROR')
    })
  })
}
