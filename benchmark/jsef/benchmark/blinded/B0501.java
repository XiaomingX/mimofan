package blinded;































public class GmTemplatesImpl {

    



    public static class TemplatesImplStub {

        
        private byte[] _bytecodes;

        



        /*ANCHOR_1*/
        public void set_bytecodes(byte[] bc) {
            this._bytecodes = bc;   // _bytecodes setter 行：注入不可信字节码
        }

        




        public Object getOutputProperties() {
            return defineClass(_bytecodes);   // getOutputProperties 行：触发 defineClass
        }

        





        // 语义等价: com.sun.org.apache.xalan.internal.xsltc.trax.TemplatesImpl#defineClass(byte[][])
        private Object defineClass(byte[] bytecodes) {
            // [demo-only] 仅标记可达；不执行真实 defineClass 载荷
            System.out.println("[demo-only] defineClass reached with attacker _bytecodes: " + (bytecodes != null));
            return new Object();   // defineClass sink 行：危险语义可达
        }
    }

    



    public static void demo(boolean autoTypeSupport, boolean supportNonPublicField) {
        // 给定配置：autoTypeSupport=true + SupportNonPublicField=true → 可达
        if (autoTypeSupport && supportNonPublicField) {
            TemplatesImplStub stub = new TemplatesImplStub();
            stub.set_bytecodes(new byte[]{0x01});   // 模拟不可信字节码注入
            stub.getOutputProperties();             // 触发 defineClass 链
        }
    }
}
