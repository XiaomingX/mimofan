
package blinded;

import org.apache.commons.compress.archivers.tar.TarArchiveEntry;
import org.apache.commons.compress.archivers.tar.TarArchiveInputStream;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;










public class TarSlipBx {

    


    static void untar(InputStream in, String dest) throws IOException {
        // 库行为声明：Apache Commons Compress 的 TAR 流读取器
        TarArchiveInputStream tar = new TarArchiveInputStream(in);
        TarArchiveEntry e;
        while ((e = tar.getNextTarEntry()) != null) {
            Path out = Paths.get(dest, e.getName()); // e.getName() 未校验，可含 ../ 或绝对路径
            /*ANCHOR_1*/
            Files.copy(tar, out); // 写入 out：穿越目录时覆盖写目标目录之外的文件
        }
    }
}
