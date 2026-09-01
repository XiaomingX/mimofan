package blinded;

import java.io.IOException;
import java.io.InputStream;








public class StreamLeakNoFinallyBy {

    public String read(String path) throws IOException {
        try (InputStream in = open(path)) {
            byte[] buf = new byte[1024];
            int n = in.read(buf);
            /*ANCHOR_1*/
            return n > 0 ? new String(buf, 0, n) : "";
        }
    }

    private InputStream open(String path) throws IOException {
        return new java.io.ByteArrayInputStream("localhost-demo".getBytes());
    }

    public static void main(String[] args) throws IOException {
        System.out.println(new StreamLeakNoFinallyBy().read("/tmp/demo.txt"));
    }
}
