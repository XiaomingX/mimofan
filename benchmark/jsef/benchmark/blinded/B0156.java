package blinded;

import java.io.FileInputStream;
import java.io.IOException;
import java.nio.file.Paths;









public class L0PathDirectBy {

    private static final String BASE_DIR = "/var/local/bench/data";

    




    public void run(String userInput) throws IOException {
        String resolved = Paths.get(BASE_DIR, userInput).normalize().toString();
        if (!resolved.startsWith(BASE_DIR)) {
            throw new SecurityException("path traversal blocked: " + userInput);
        }
        /*ANCHOR_1*/
        FileInputStream fis = new FileInputStream(resolved);
    }

    public static void main(String[] args) throws IOException {
        new L0PathDirectBy().run("localhost-demo.txt");
    }
}
