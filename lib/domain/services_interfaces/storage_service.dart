abstract class IStorageService {
  Future<void> atomicWrite(String filePath, String content);
}
