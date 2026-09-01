package blinded;

import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;













public class StreamLeakNoFinally {

    




    public String read(String path) throws IOException {
        InputStream in = new FileInputStream(path); // 语义演示
        byte[] buf = new byte[1024];
        int n = in.read(buf);
        /*ANCHOR_1*/
        return n > 0 ? new String(buf, 0, n) : ""; // 缺陷：in 未关闭
    }

    public static void main(String[] args) throws IOException {
        System.out.println(new StreamLeakNoFinally().read("/tmp/demo.txt"));
    }
}
