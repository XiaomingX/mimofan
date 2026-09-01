
package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;






@RestController
public class FileUploadControllerBy {

    private final StorageServiceBy storageService;

    public FileUploadControllerBy(StorageServiceBy storageService) {
        this.storageService = storageService;
    }

    @PostMapping("/api/v1/files/upload")
    public String upload(@RequestParam("name") String name,
                         @RequestParam("content") String content,
                         @RequestParam("mode") String mode) {
        // 安全：用户提交的 mode 被忽略，内部强制安全权限
        /*ANCHOR_1*/
        return storageService.store(name, content, mode);
    }
}
