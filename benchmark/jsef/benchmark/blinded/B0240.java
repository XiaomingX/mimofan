package blinded;

import java.io.FileInputStream;
import java.io.IOException;












public class StreamResourceLeak_By {

    




    public void read(String path) throws IOException {
        try (FileInputStream in = new FileInputStream(path)) {
            int b = in.read(); // 读取首字节仅作演示
            /*ANCHOR_1*/
        }
    }

    public static void main(String[] args) throws IOException {
        new StreamResourceLeak_By().read("/tmp/localhost-demo.txt");
    }
}
