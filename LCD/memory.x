/* Memory layout for STM32U545RE - used by cortex-m-rt's link.x include */
MEMORY
{
  FLASH (rx)  : ORIGIN = 0x08000000, LENGTH = 512K
  RAM   (rwx) : ORIGIN = 0x20000000, LENGTH = 160K
}

/* Keep this file minimal: only MEMORY regions. The sections/layout come from
   cortex-m-rt's link.x which `link-arg=-Tlink.x` points to. */
