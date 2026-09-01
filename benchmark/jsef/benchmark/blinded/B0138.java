package blinded;

import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.io.InvalidClassException;
import java.io.ObjectInputStream;
import java.util.Set;






public class BxDeserOfTrustBy {

    private static final Set<String> ALLOWED = Set.of(
            "com.jsef.benchmark.dto.ByPayload");

    


    public static Object deserialize(byte[] data) throws Exception {
        // source：信任边界外的不可信字节流（仍被允许，但受类型白名单约束）
        try (ObjectInputStream ois = new LookAheadOis(data)) {
            /*ANCHOR_1*/
            return ois.readObject();
        }
    }

    static class LookAheadOis extends ObjectInputStream {
        LookAheadOis(byte[] data) throws IOException {
            super(new ByteArrayInputStream(data));
        }
        @Override
        protected Class<?> resolveClass(java.io.ObjectStreamClass desc)
                throws IOException, ClassNotFoundException {
            if (!ALLOWED.contains(desc.getName())) {
                throw new InvalidClassException("类型不在白名单: " + desc.getName());
            }
            return super.resolveClass(desc);
        }
    }
}
