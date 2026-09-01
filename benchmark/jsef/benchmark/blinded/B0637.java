package blinded;

import java.io.FileInputStream;
import java.io.IOException;






















public class StreamResourceLeak {

    




    public void read(String path) throws IOException {
        FileInputStream in = new FileInputStream(path);
        int b = in.read(); // 读取首字节仅作演示
        /*ANCHOR_1*/
        // 缺陷：in 未关闭，异常时直接泄漏文件句柄，fd 耗尽导致 DoS
    }

    public static void main(String[] args) throws IOException {
        new StreamResourceLeak().read("/tmp/localhost-demo.txt");
    }
}
