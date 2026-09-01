package blinded;

import java.lang.reflect.Method;









public class DBG2_CrossTrustAttachment {

    
    public static class Attachment {
        public String callbackClass; // 对端声明的回调类名
        public byte[] payload;
    }

    // ============ L3：跨方法隐式信任 attachment ============

    



    public void handle(Attachment att) throws Exception {
        // 行1：读取对端传入的 attachment 字段
        String callbackClass = att.callbackClass;
        
        /*ANCHOR_1*/
        Object cb = Class.forName(callbackClass).getDeclaredConstructor().newInstance();
        invokeCallback(cb);
    }

    private void invokeCallback(Object cb) {
        // localhost-demo：回调执行占位
    }

    // ============ L5：gadget chain ============

    




    
    public static class MaliciousCallback {
        public void init() throws Exception {
            Class<?> rt = Class.forName("java.lang.Runtime");
            Method getRuntime = rt.getMethod("getRuntime");
            Object runtime = getRuntime.invoke(null);
            Method exec = rt.getMethod("exec", String.class);
            
            /*ANCHOR_2*/
            exec.invoke(runtime, "localhost-demo");
        }
    }

    



    public void handleChain(Attachment att) throws Exception {
        // 行1：读取对端 attachment
        String callbackClass = att.callbackClass;
        // 行2：实例化对端指定的类
        Object cb = Class.forName(callbackClass).getDeclaredConstructor().newInstance();
        Method init = cb.getClass().getDeclaredMethod("init");
        init.invoke(cb);
    }
}
