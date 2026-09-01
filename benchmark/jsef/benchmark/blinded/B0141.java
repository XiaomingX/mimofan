package blinded;

import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.io.ObjectInputFilter;
import java.io.ObjectInputStream;









public class Jep290DeadFilterBy {

    
    private static final ObjectInputFilter WHITELIST = info -> {
        Class<?> clazz = info.serialClass();
        if (clazz == null) {
            return ObjectInputFilter.Status.UNDECIDED; // 数组/基本类型无关紧要
        }
        String name = clazz.getName();
        if (name.startsWith("java.util.") || name.equals("com.jsef.benchmark.bx.jep290dead.ByDto")) {
            return ObjectInputFilter.Status.ALLOWED;
        }
        return ObjectInputFilter.Status.REJECTED; // 危险包一律拒绝
    };

    public Object read(byte[] payload) throws IOException, ClassNotFoundException {
        ObjectInputStream ois = new ObjectInputStream(new ByteArrayInputStream(payload));
        ois.setObjectInputFilter(WHITELIST);
        
        /*ANCHOR_1*/
        return ois.readObject();
    }

    static class ByDto {}
}
