import writerCoreBridgeTest from './WriterCoreBridgeTest'
import coreDtosTest from './CoreDtosTest'
import editorSessionPatchTest from './EditorSessionPatchTest'
import inputDeviceMappingTest from './InputDeviceMappingTest'
import shareDispatchRoutingTest from './ShareDispatchRoutingTest'

export default function testsuite() {
  writerCoreBridgeTest()
  coreDtosTest()
  editorSessionPatchTest()
  inputDeviceMappingTest()
  shareDispatchRoutingTest()
}
