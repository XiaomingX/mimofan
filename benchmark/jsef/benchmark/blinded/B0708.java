package blinded;

import java.lang.AutoCloseable;















public class TCM2_InheritanceBypass {

    // 受信任的父类（某JSON反序列化库 1.2.68 期望父类 AutoCloseable/Throwable 的抽象）
    public interface TrustedBase extends AutoCloseable {
        void by();
    }

    // 危险子类：它是 TrustedBase 的合法子类，但 close() 内含危险 sink
    public static class Evil implements TrustedBase {
        @Override
        public void by() {
            System.out.println("Evil.by (benign)");
        }

        @Override
        public void close() throws Exception {
            
            Runtime.getRuntime().exec("localhost-demo"); // 仅占位，不连真实远端
        }
    }

    
    public void handle(String typeName) throws Exception {
        Class<?> c = Class.forName(typeName);
        if (TrustedBase.class.isAssignableFrom(c)) { // 不安全的父类白名单
            TrustedBase obj = (TrustedBase) c.getDeclaredConstructor().newInstance();
            /*ANCHOR_1*/
            obj.close(); // 隐式触发危险子类逻辑
        }
    }
}
