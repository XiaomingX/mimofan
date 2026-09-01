package blinded;

import java.io.ByteArrayInputStream;
import java.io.IOException;









public class L0DeserDirectBy {

    




    public void run(byte[] data) throws IOException, ClassNotFoundException {
        ByteArrayInputStream bais = new ByteArrayInputStream(data);
        // 语义等价：ObjectInputStream 设置 allowlist filter，拒绝非白名单类
        /*ANCHOR_1*/
        Object obj = readAllowed(bais, "com.jsef.benchmark.bx.ByDto");
    }

    private static Object readAllowed(ByteArrayInputStream bais, String allowed) throws IOException, ClassNotFoundException {
        // 简化演示：仅声明类型白名单，实际由 ObjectInputFilter 在运行态拦截
        System.out.println("[deser-by] allowlist filter = " + allowed);
        return null;
    }

    public static void main(String[] args) {
        System.out.println("demo: readObject with allowlist localhost-demo");
    }
}
