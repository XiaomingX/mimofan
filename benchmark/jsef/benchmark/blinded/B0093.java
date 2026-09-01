package blinded;

import java.lang.reflect.Method;
import java.util.Arrays;
import java.util.List;








public class DBG2_CrossTrustAttachment_By {

    public static class Attachment {
        public String callbackClass;
        public byte[] payload;
    }

    
    private static final List<String> ALLOWED_CALLBACKS =
            Arrays.asList("com.jsef.benchmark.bx.dbg.ByCallbackA");

    // ============ L3：attachment 类名走服务端白名单 ============

    



    public void handle(Attachment att) throws Exception {
        String callbackClass = att.callbackClass;
        
        /*ANCHOR_1*/
        if (!ALLOWED_CALLBACKS.contains(callbackClass)) {
            throw new SecurityException("callback class not allowed: " + callbackClass);
        }
        Object cb = Class.forName(callbackClass).getDeclaredConstructor().newInstance();
        invokeCallback(cb);
    }

    private void invokeCallback(Object cb) {
        // localhost-demo：回调执行占位
    }

    // ============ L5：回调实例由服务端预置 ============

    



    public void handleChain(Attachment att) throws Exception {
        
        /*ANCHOR_2*/
        Object cb = new ByCallbackA();
        Method init = cb.getClass().getDeclaredMethod("init");
        init.invoke(cb);
    }

    
    public static class ByCallbackA {
        public void init() {
            // localhost-demo：仅执行安全逻辑，无 Runtime.exec 可达性
        }
    }
}
