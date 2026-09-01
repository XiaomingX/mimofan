
package blinded;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;







public class ZipSlipBy {

    


    static void unzip(InputStream zip, String destDir) throws IOException {
        ZipInputStream zis = new ZipInputStream(zip);
        ZipEntry entry;
        File dest = new File(destDir).getCanonicalFile();
        while ((entry = zis.getNextEntry()) != null) {
            File out = new File(dest, entry.getName()).getCanonicalFile();
            /*ANCHOR_1*/
            if (!out.toPath().startsWith(dest.toPath())) {
                continue; // 路径穿越，跳过
            }
            FileOutputStream fos = new FileOutputStream(out);
            byte[] buf = new byte[4096];
            int n;
            while ((n = zis.read(buf)) > 0) fos.write(buf, 0, n);
            fos.close();
        }
    }
}
