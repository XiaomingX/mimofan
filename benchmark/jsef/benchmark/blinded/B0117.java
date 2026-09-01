package blinded;






















public class GmTemplatesImplBy {

    


    public static class TemplatesImplStub {

        private byte[] _bytecodes;

        



        public void set_bytecodes(byte[] bc, boolean supportNonPublicField) {
            if (!supportNonPublicField) {
                // 安全处理：未开启 SupportNonPublicField，拒绝写入私有危险字段
                System.out.println("[demo-only] SupportNonPublicField off: reject _bytecodes write");
                return;
            }
            this._bytecodes = bc;
        }

        



        public Object getOutputProperties() {
            return defineClass(_bytecodes);
        }

        private Object defineClass(byte[] bytecodes) {
            if (bytecodes == null) {
                // 安全态：无不可信字节码，defineClass 不会加载恶意类
                System.out.println("[demo-only] defineClass by: no attacker bytecodes");
                return new Object();
            }
            System.out.println("[demo-only] defineClass reached with attacker _bytecodes");
            return new Object();
        }
    }

    


    /*ANCHOR_1*/
    public static void byDemo(boolean autoTypeSupport, boolean supportNonPublicField) {
        // 未开启 SupportNonPublicField（或字段只读校验）→ 不可达
        TemplatesImplStub stub = new TemplatesImplStub();
        stub.set_bytecodes(new byte[]{0x01}, supportNonPublicField);   // 安全态：写入被拒
        stub.getOutputProperties();
    }
}
