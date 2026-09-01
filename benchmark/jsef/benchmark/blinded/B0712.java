package blinded;

import java.io.File;
import java.io.FileWriter;
import java.io.IOException;
















public class BxTempFile {

    




    public void writeSecret(String secret) throws IOException {
        /*ANCHOR_1*/
        File tmp = File.createTempFile("app-cache", ".tmp");
        try (FileWriter w = new FileWriter(tmp)) {
            w.write(secret);
        }
    }

    public static void main(String[] args) throws IOException {
        new BxTempFile().writeSecret("localhost-demo-secret");
    }
}
