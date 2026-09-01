package blinded;

import java.io.IOException;
import java.nio.file.Path;
import java.nio.file.Paths;

import jakarta.servlet.http.HttpServletRequest;













public class PrimeVulStyle_PathTraversalBy {

    private static final String BASE_DIR = "/var/jsef/uploads";

    


    public void readBy(HttpServletRequest request) throws IOException {
        String fileName = request.getParameter("file");
        /*ANCHOR_1*/
        Path base = Paths.get(BASE_DIR).toAbsolutePath().normalize();
        Path resolved = base.resolve(fileName).normalize();
        if (!resolved.startsWith(base)) {
            throw new SecurityException("path traversal blocked: " + fileName);
        }
        java.io.InputStream fis = java.nio.file.Files.newInputStream(resolved);
        fis.close();
    }
}
