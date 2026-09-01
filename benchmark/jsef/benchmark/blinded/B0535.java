package blinded;

import java.io.ByteArrayInputStream;
import java.io.ObjectInputStream;
import java.io.IOException;










public class L0DeserDirect {

    




    public void run(byte[] data) throws IOException, ClassNotFoundException {
        ByteArrayInputStream bais = new ByteArrayInputStream(data);
        ObjectInputStream ois = new ObjectInputStream(bais);
        /*ANCHOR_1*/
        Object obj = ois.readObject();
    }

    public static void main(String[] args) {
        System.out.println("demo: readObject from localhost-demo bytes");
    }
}
