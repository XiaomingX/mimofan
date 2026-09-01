package blinded;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;














public class BxTempFileRace {

    




    public void store(String data) throws IOException {
        File tmp = File.createTempFile("report", ".tmp");
        tmp.setReadable(true, false); // 放宽权限，引入 TOCTOU 窗口
        tmp.setWritable(true, false);
        /*ANCHOR_1*/
        try (FileOutputStream out = new FileOutputStream(tmp)) {
            out.write(data.getBytes());
        }
    }

    public static void main(String[] args) throws IOException {
        new BxTempFileRace().store("localhost-demo-data");
    }
}
