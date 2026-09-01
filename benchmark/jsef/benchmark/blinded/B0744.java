
package blinded;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;









public class ZipSlip {

    


    static void unzip(InputStream zip, String destDir) throws IOException {
        ZipInputStream zis = new ZipInputStream(zip);
        ZipEntry entry;
        while ((entry = zis.getNextEntry()) != null) {
            /*ANCHOR_1*/
            File out = new File(destDir, entry.getName()); // 未校验 entry name，可穿越
            FileOutputStream fos = new FileOutputStream(out);
            byte[] buf = new byte[4096];
            int n;
            while ((n = zis.read(buf)) > 0) fos.write(buf, 0, n);
            fos.close();
        }
    }
}
