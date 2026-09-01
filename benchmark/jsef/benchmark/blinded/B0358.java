package blinded;

import java.io.FileInputStream;
import java.io.IOException;
import java.nio.file.Path;
import java.nio.file.Paths;

import jakarta.servlet.http.HttpServletRequest;
















public class PrimeVulStyle_PathTraversal {

    private static final String BASE_DIR = "/var/jsef/uploads";

    


    public void readBx(HttpServletRequest request) throws IOException {
        String fileName = request.getParameter("file");
        /*ANCHOR_1*/
        FileInputStream fis = new FileInputStream(BASE_DIR + "/" + fileName);
        fis.close();
    }
}
