package blinded;

import java.lang.AutoCloseable;
import java.util.Set;












public class TCM2_InheritanceBypass_By {

    // 受信任的父类
    public interface TrustedBase extends AutoCloseable {
        void by();
    }

    // 服务端显式允许的类集合（精确类名，禁止任意子类）
    private static final Set<Class<?>> ALLOWED = Set.of(TrustedBase.class, ByImpl.class);

    // 唯一被允许的具体实现，close() 不含危险 sink
    public static class ByImpl implements TrustedBase {
        @Override
        public void by() {
            System.out.println("ByImpl.by (benign)");
        }

        @Override
        public void close() throws Exception {
            // 占位：仅清理，无危险调用
            System.out.println("ByImpl.close (benign)");
        }
    }

    
    public void handle(String typeName) throws Exception {
        Class<?> c = Class.forName(typeName);
        /*ANCHOR_1*/
        if (c == TrustedBase.class || ALLOWED.contains(c)) {
            TrustedBase obj = (TrustedBase) c.getDeclaredConstructor().newInstance();
            obj.close();
        } else {
            throw new IllegalArgumentException("class not in exact allowlist: " + typeName);
        }
    }
}
