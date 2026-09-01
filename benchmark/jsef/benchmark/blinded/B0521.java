package blinded;

import java.io.ByteArrayInputStream;
import java.io.ObjectInputStream;











public class BxDeserOfTrust {

    


    public static Object deserialize(byte[] data) throws Exception {
        // source：信任边界外的不可信字节流
        /*ANCHOR_1*/
        try (ObjectInputStream ois = new ObjectInputStream(new ByteArrayInputStream(data))) {
            return ois.readObject();
        }
    }
}
